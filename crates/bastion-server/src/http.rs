//! The HTTP API (D28) and its event stream (D29). Each route calls one
//! function in `api`. Every request carries a person's token, and the
//! person's role decides what they may do (D31, `access`).

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Extension, MatchedPath, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use bastion_core::{Event as JobEvent, Pipeline};
use bastion_protocol::{
    Change, ConstraintView, EscapeReport, HeldCallRef, HostView, LogRequest, NewEval, NotifySet,
    NotifyView, ProposalDecision, SettingsView, Workspace,
};
use futures_util::{Stream, stream};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;

use bastion_core::people::{Act, Person, Role};

use crate::access::{self, Refusal};
use crate::api::settings::FromIssue;
use crate::api::{self, factory::NOT_STARTED};
use crate::factory::Submission;
use crate::server::Server;

#[derive(Clone)]
struct App {
    server: Arc<Server>,
    /// The token in `data/token`, which is the owner's.
    token: Arc<str>,
    owner: Person,
}

/// The API for `server`. `token` is the owner's, and `owner` names them.
pub fn router(server: Arc<Server>, token: &str, owner: &str) -> Router {
    let app = App {
        server,
        token: token.into(),
        owner: Person {
            name: owner.into(),
            role: Role::Admin,
        },
    };
    routes()
        .route_layer(middleware::from_fn_with_state(app.clone(), authorized))
        .with_state(app)
}

fn routes() -> Router<App> {
    Router::new()
        .route("/events", get(events))
        .route("/me", get(me))
        .route("/engine", get(engine))
        .route("/doctor", get(doctor))
        .route("/stack", get(stack_status))
        .route("/stack/start", post(stack_start))
        .route("/stack/stop", post(stack_stop))
        .route("/console", get(console))
        .route("/console/logs", post(container_logs))
        .route("/console/{name}/stop", post(container_stop))
        .route("/console/{name}/start", post(container_start))
        .merge(factory_routes())
        .merge(govern_routes())
        .merge(settings_routes())
        .merge(people_routes())
        .merge(provider_routes())
        .merge(workflow_routes())
}

fn people_routes() -> Router<App> {
    Router::new()
        .route("/people", get(people).post(person_add))
        .route("/people/{name}/role", put(person_role))
        .route("/people/{name}/tokens", post(token_issue))
        .route("/tokens", get(tokens))
        .route("/tokens/{id}", delete(token_revoke))
}

fn provider_routes() -> Router<App> {
    Router::new()
        .route("/providers", get(providers))
        .route("/providers/hosts", get(hosts).put(hosts_set))
        .route(
            "/providers/{id}/key",
            put(provider_key_set).delete(provider_key_remove),
        )
        .route("/providers/openai-codex/sign-in", post(codex_sign_in))
        .route(
            "/providers/openai-codex/sign-in/poll",
            post(codex_sign_in_poll),
        )
        .route("/providers/openai-codex/sign-out", post(codex_sign_out))
}

/// Workflows (D35). Copying, saving and deleting are an admin's (D31).
fn workflow_routes() -> Router<App> {
    Router::new()
        .route("/workflows", get(workflows).post(workflow_copy))
        .route(
            "/workflows/{id}",
            put(workflow_save).delete(workflow_delete),
        )
}

async fn workflows(State(app): State<App>) -> Response {
    answer(api::workflows::workflows(&app.server.factory).await)
}

#[derive(Deserialize)]
struct CopyWorkflow {
    from: String,
    name: String,
}

async fn workflow_copy(State(app): State<App>, Json(b): Json<CopyWorkflow>) -> Response {
    let copied = api::workflows::workflow_copy(&app.server.factory, b.from, b.name).await;
    if copied.is_ok() {
        app.server.events.publish(Change::Job { id: String::new() });
    }
    answer(copied)
}

async fn workflow_save(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(w): Json<bastion_protocol::WorkflowView>,
) -> Response {
    let saved = api::workflows::workflow_save(&app.server.factory, id, w).await;
    if saved.is_ok() {
        app.server.events.publish(Change::Job { id: String::new() });
    }
    answer(saved)
}

async fn workflow_delete(State(app): State<App>, Path(id): Path<String>) -> Response {
    read_again(
        &app,
        api::workflows::workflow_delete(&app.server.factory, id).await,
    )
}

fn factory_routes() -> Router<App> {
    Router::new()
        .route("/workspaces", get(workspaces).post(workspace_add))
        .route("/workspaces/draft", post(workspace_draft))
        .route("/workspaces/{name}", put(workspace_edit))
        .route("/jobs", get(jobs).post(job_submit))
        .route("/costs", get(costs))
        .route("/sessions/{id}", get(session))
        .route("/proposals", get(proposals))
        .route("/proposals/decide", post(proposal_decide))
        .route("/verification", get(verification))
        .route("/escapes", post(escape_add))
        .route("/jobs/{id}", get(job))
        .route("/jobs/{id}/approve", post(job_approve))
        .route("/jobs/{id}/comment", post(job_comment))
        .route("/jobs/{id}/parts", post(job_parts))
        .route(bastion_protocol::api::MEDIA_ROUTE, get(job_media))
        .route("/jobs/{id}/stop", post(job_stop))
        .route("/jobs/{id}/retry", post(job_retry))
        .route("/jobs/{id}/extend", post(job_extend))
        .route("/dials", get(dials))
        .route("/dials/{stage}", put(dials_set))
        .route("/evals", get(evals).post(eval_create))
        .route("/evals/{id}", get(eval))
        .route("/evals/{id}/apply", post(eval_apply))
}

fn govern_routes() -> Router<App> {
    Router::new()
        .route("/held/allow", post(held_allow))
        .route("/held/refuse", post(held_refuse))
        .route("/constraints", get(constraints).post(constraint_add))
        .route("/constraints/{id}/move", post(constraint_move))
        .route("/capabilities", get(capabilities))
        .route("/capabilities/move", post(capability_move))
        .route("/capabilities/factory", get(factory_capabilities))
        .route("/charters", get(charters))
        .route("/audit", get(audit))
        .route("/audit/export", post(audit_export))
}

fn settings_routes() -> Router<App> {
    Router::new()
        .route("/settings", get(settings).put(settings_set))
        .route("/notify", get(notify).put(notify_set))
        .route("/notify/test", post(notify_test))
        .route("/settings/preset", post(settings_preset))
        .route("/settings/history", get(settings_history))
        .route("/settings/restore", post(settings_restore))
        .route("/models", get(models))
        .route("/accounts", get(accounts))
        .route("/issues/{workspace}", get(issues))
        .route("/issues/{workspace}/{number}/job", post(job_from_issue))
}

/// Find who is asking, check their role allows what the route does, and
/// hand the person to the handler.
async fn authorized(State(app): State<App>, mut request: Request, next: Next) -> Response {
    let given = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_string();
    let path = request
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_string())
        .unwrap_or_default();
    let act = access::act_for(request.method(), &path);
    let person = match access::person(&app.server, &app.owner, &app.token, &given).await {
        Ok(person) => person,
        Err(why) => return refused(why),
    };
    let did = format!("{} {}", request.method(), request.uri().path());
    let response = match access::check(&person, act) {
        Ok(()) => {
            request.extensions_mut().insert(person.clone());
            next.run(request).await
        }
        Err(why) => refused(why),
    };
    if let Some(kind) = audit_kind(act, response.status()) {
        note_act(&app, kind, &person, &did);
    }
    engine_down(&app, response)
}

/// A request that failed while the engine link is down failed because of
/// the engine, not the request: 503, not 400.
fn engine_down(app: &App, mut response: Response) -> Response {
    let down = app.server.engine.link() != bastion_protocol::Link::Up;
    if down && response.status() == StatusCode::BAD_REQUEST {
        *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    }
    response
}

/// What the audit log keeps of a request: every change a person makes, and
/// every attempt their role refused (D31, D33). Reads are not kept.
fn audit_kind(act: Act, status: StatusCode) -> Option<&'static str> {
    if status == StatusCode::FORBIDDEN {
        Some("refused")
    } else if act != Act::Read && status.is_success() {
        Some("act")
    } else {
        None
    }
}

/// Note what `person` did, or tried to do, in the audit log.
fn note_act(app: &App, kind: &str, person: &Person, did: &str) {
    let Some(factory) = app.server.factory.get() else {
        return;
    };
    if let Ok(log) = factory.gate().audit() {
        log.note(bastion_core::audit::Record {
            kind: kind.into(),
            subject: did.into(),
            detail: format!("{} ({:?})", person.name, person.role),
        });
    }
}

fn refused(why: Refusal) -> Response {
    let (status, error) = match why {
        Refusal::Unknown => (StatusCode::UNAUTHORIZED, "no valid token".to_string()),
        Refusal::NotReady => (
            StatusCode::SERVICE_UNAVAILABLE,
            "the server's people are not open yet; start the stack".to_string(),
        ),
        Refusal::Forbidden(why) => (StatusCode::FORBIDDEN, why),
        Refusal::NoJob(why) => (StatusCode::NOT_FOUND, why),
    };
    (status, Json(json!({ "error": error }))).into_response()
}

/// Who the token belongs to, and their role.
async fn me(Extension(person): Extension<Person>) -> Response {
    Json(api::people::view(&person)).into_response()
}

/// Run `then` if `person` may act on job `id`.
async fn on_job(
    app: &App,
    person: &Person,
    id: &str,
    then: impl std::future::Future<Output = Response>,
) -> Response {
    match access::on_job(&app.server, person, id).await {
        Ok(()) => then.await,
        Err(why) => refused(why),
    }
}

/// A result as a response: the value, or the reason it was refused. A
/// factory that has not started yet is unavailable rather than refused.
fn answer<T: Serialize>(result: Result<T, String>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) if e == NOT_STARTED => {
            (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": e }))).into_response()
        }
        Err(e) if crate::store::is_no_job(&e) => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": e }))).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

/// A result whose error is typed, such as the stack's `StartRefused`.
fn typed<T: Serialize, E: Serialize>(result: Result<T, E>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => (StatusCode::CONFLICT, Json(e)).into_response(),
    }
}

/// A result that cannot fail.
fn always<T: Serialize>(result: Result<T, ()>) -> Response {
    typed(result)
}

async fn engine(State(app): State<App>) -> Response {
    always(api::engine::engine_status(&app.server.engine).await)
}

async fn doctor(State(app): State<App>) -> Response {
    let s = &app.server;
    always(api::doctor::doctor(&s.machine, &s.engine, &s.stack.checkout).await)
}

async fn stack_status(State(app): State<App>) -> Response {
    answer(api::stack::stack_status(
        &app.server.stack,
        &app.server.machine,
    ))
}

async fn stack_start(State(app): State<App>) -> Response {
    let s = &app.server;
    typed(api::stack::stack_start(&s.stack, &s.machine, &s.engine).await)
}

async fn stack_stop(State(app): State<App>) -> Response {
    answer(api::stack::stack_stop(&app.server.stack, &app.server.machine).await)
}

async fn console(State(app): State<App>) -> Response {
    answer(api::console::console(&app.server.engine).await)
}

async fn container_logs(State(app): State<App>, Json(r): Json<LogRequest>) -> Response {
    answer(api::console::container_logs(&app.server.engine, r).await)
}

async fn container_stop(State(app): State<App>, Path(name): Path<String>) -> Response {
    answer(api::console::container_stop(&app.server.engine, name).await)
}

async fn container_start(State(app): State<App>, Path(name): Path<String>) -> Response {
    answer(api::console::container_start(&app.server.engine, name).await)
}

async fn workspaces(State(app): State<App>) -> Response {
    answer(api::factory::workspaces(&app.server.factory).await)
}

async fn workspace_add(State(app): State<App>, Json(w): Json<Workspace>) -> Response {
    let s = &app.server;
    answer(api::factory::workspace_add(&s.factory, &s.machine, w).await)
}

async fn workspace_draft(
    State(app): State<App>,
    Json(r): Json<api::factory::DraftRequest>,
) -> Response {
    let s = &app.server;
    answer(api::factory::workspace_draft(&s.factory, &s.machine, r).await)
}

async fn workspace_edit(
    State(app): State<App>,
    Path(name): Path<String>,
    Json(edit): Json<api::factory::WorkspaceSettings>,
) -> Response {
    let s = &app.server;
    answer(api::factory::workspace_edit(&s.factory, &s.machine, &name, edit).await)
}

async fn session(State(app): State<App>, Path(id): Path<String>) -> Response {
    answer(api::sessions::session(&app.server.factory, id).await)
}

async fn proposals(State(app): State<App>) -> Response {
    answer(api::learn::proposals(&app.server.factory).await)
}

async fn proposal_decide(State(app): State<App>, Json(d): Json<ProposalDecision>) -> Response {
    answer(api::learn::decide(&app.server.factory, d).await)
}

async fn verification(State(app): State<App>) -> Response {
    answer(api::verification::verification(&app.server.factory).await)
}

async fn escape_add(State(app): State<App>, Json(r): Json<EscapeReport>) -> Response {
    answer(api::verification::escape_add(&app.server.factory, r).await)
}

async fn costs(State(app): State<App>) -> Response {
    answer(api::costs::costs(&app.server.factory).await)
}

async fn jobs(
    State(app): State<App>,
    Extension(person): Extension<Person>,
    Query(page): Query<api::Page>,
) -> Response {
    let jobs = api::factory::jobs(&app.server.factory, page).await;
    answer(jobs.map(|all| {
        all.into_iter()
            .map(|j| access::as_seen_by(&person, j))
            .collect::<Vec<_>>()
    }))
}

#[derive(Deserialize)]
struct Submit {
    workspace: String,
    request: String,
    pipeline: Option<Pipeline>,
    /// A workflow's id (D35), which takes the place of `pipeline`.
    workflow: Option<String>,
    /// `provider/model` for every turn.
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    criteria: Option<String>,
    #[serde(default)]
    links: Vec<String>,
    #[serde(default)]
    screenshots: Vec<bastion_protocol::api::Screenshot>,
}

async fn job_submit(
    State(app): State<App>,
    Extension(person): Extension<Person>,
    Json(b): Json<Submit>,
) -> Response {
    let names: Vec<String> = b.screenshots.iter().map(|s| s.name.clone()).collect();
    let asked = bastion_core::request::Request {
        text: &b.request,
        kind: b.kind.as_deref(),
        criteria: b.criteria.as_deref(),
        links: &b.links,
        screenshots: &names,
    };
    let request = match bastion_core::request::compose(&asked) {
        Ok(text) => text,
        Err(e) => return answer::<String>(Err(e)),
    };
    let how = Submission {
        pipeline: b.pipeline.unwrap_or_default(),
        workflow: b.workflow,
        model: b.model.filter(|m| !m.trim().is_empty()),
        requester: Some(person.name),
        criteria: b.criteria.filter(|c| !c.trim().is_empty()),
        screenshots: b
            .screenshots
            .into_iter()
            .map(|s| (s.name, s.bytes))
            .collect(),
        ..Submission::default()
    };
    let f = &app.server.factory;
    answer(api::factory::job_submit(f, b.workspace, request, how).await)
}

async fn job(
    State(app): State<App>,
    Extension(person): Extension<Person>,
    Path(id): Path<String>,
) -> Response {
    let detail = api::factory::job(&app.server.factory, id).await;
    answer(detail.map(|mut d| {
        d.job = access::as_seen_by(&person, d.job);
        d
    }))
}

async fn job_approve(
    State(app): State<App>,
    Extension(person): Extension<Person>,
    Path(id): Path<String>,
) -> Response {
    let by = person.name.clone();
    match access::approve(&app.server, &person, &id).await {
        Ok(()) => {
            answer(api::factory::job_decide(&app.server.factory, id, JobEvent::Approved, by).await)
        }
        Err(why) => refused(why),
    }
}

#[derive(Deserialize)]
struct Text {
    text: String,
}

/// A person's decision on job `id`, under their access to it.
async fn decide_as(app: &App, person: &Person, id: String, event: JobEvent) -> Response {
    let by = person.name.clone();
    let decide = api::factory::job_decide(&app.server.factory, id.clone(), event, by);
    on_job(app, person, &id, async { answer(decide.await) }).await
}

async fn job_comment(
    State(app): State<App>,
    Extension(person): Extension<Person>,
    Path(id): Path<String>,
    Json(b): Json<Text>,
) -> Response {
    let by = person.name.clone();
    let comment = api::factory::job_comment(&app.server.factory, id.clone(), b.text, by);
    on_job(&app, &person, &id, async { answer(comment.await) }).await
}

#[derive(Deserialize)]
struct Parts {
    parts: Vec<String>,
}

async fn job_parts(
    State(app): State<App>,
    Extension(person): Extension<Person>,
    Path(id): Path<String>,
    Json(b): Json<Parts>,
) -> Response {
    decide_as(&app, &person, id, JobEvent::PartsSet { parts: b.parts }).await
}

/// A job's kept screenshot or recording, as its bytes.
async fn job_media(State(app): State<App>, Path((id, name)): Path<(String, String)>) -> Response {
    let found = app
        .server
        .factory
        .get()
        .and_then(|f| f.media_path(&id, &name));
    let bytes = found.and_then(|p| app.server.machine.read_bytes(&p).ok());
    match bytes {
        Some(b) => {
            let kind = bastion_core::media::content_type(&name);
            ([(axum::http::header::CONTENT_TYPE, kind)], b).into_response()
        }
        None => {
            let error = format!("job {id} kept no {name}");
            (StatusCode::NOT_FOUND, Json(json!({ "error": error }))).into_response()
        }
    }
}

async fn job_stop(
    State(app): State<App>,
    Extension(person): Extension<Person>,
    Path(id): Path<String>,
) -> Response {
    let stop = api::factory::job_stop(&app.server.factory, id.clone());
    on_job(&app, &person, &id, async { answer(stop.await) }).await
}

async fn job_retry(
    State(app): State<App>,
    Extension(person): Extension<Person>,
    Path(id): Path<String>,
) -> Response {
    decide_as(&app, &person, id, JobEvent::Retried).await
}

async fn job_extend(
    State(app): State<App>,
    Extension(person): Extension<Person>,
    Path(id): Path<String>,
) -> Response {
    decide_as(&app, &person, id, JobEvent::Extended).await
}

async fn dials(State(app): State<App>) -> Response {
    answer(api::factory::dials(&app.server.factory).await)
}

#[derive(Deserialize)]
struct SetDial {
    dial: bastion_core::Dial,
}

/// A word that is not a stage, or a dial that is not `lights_on` or
/// `dark`, is refused here as a bad request.
async fn dials_set(
    State(app): State<App>,
    Path(stage): Path<bastion_core::Stage>,
    Json(b): Json<SetDial>,
) -> Response {
    read_again(
        &app,
        api::factory::dials_set(&app.server.factory, stage, b.dial).await,
    )
}

async fn evals(State(app): State<App>) -> Response {
    answer(api::evals::evals(&app.server.factory).await)
}

async fn eval_create(State(app): State<App>, Json(e): Json<NewEval>) -> Response {
    let s = &app.server;
    answer(api::evals::eval_create(&s.factory, &s.machine, e).await)
}

async fn eval(State(app): State<App>, Path(id): Path<String>) -> Response {
    answer(api::evals::eval(&app.server.factory, id).await)
}

#[derive(Deserialize)]
struct Arm {
    arm: String,
}

async fn eval_apply(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(b): Json<Arm>,
) -> Response {
    answer(api::evals::eval_apply(&app.server.factory, id, b.arm).await)
}

async fn held_allow(
    State(app): State<App>,
    Extension(person): Extension<Person>,
    Json(h): Json<HeldCallRef>,
) -> Response {
    let job = h.job.clone();
    let allow = api::govern::held_allow(&app.server.factory, h);
    on_job(&app, &person, &job, async { answer(allow.await) }).await
}

#[derive(Deserialize)]
struct RefuseCall {
    held: HeldCallRef,
    feedback: String,
}

async fn held_refuse(
    State(app): State<App>,
    Extension(person): Extension<Person>,
    Json(b): Json<RefuseCall>,
) -> Response {
    let job = b.held.job.clone();
    let refuse = api::govern::held_refuse(&app.server.factory, b.held, b.feedback);
    on_job(&app, &person, &job, async { answer(refuse.await) }).await
}

async fn constraints(State(app): State<App>) -> Response {
    answer(api::govern::constraints(&app.server.factory).await)
}

async fn constraint_add(State(app): State<App>, Json(c): Json<ConstraintView>) -> Response {
    answer(api::govern::constraint_add(&app.server.factory, c).await)
}

#[derive(Deserialize)]
struct Up {
    up: bool,
}

async fn constraint_move(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(b): Json<Up>,
) -> Response {
    answer(api::govern::constraint_move(&app.server.factory, id, b.up).await)
}

async fn capabilities(State(app): State<App>) -> Response {
    answer(api::govern::capabilities(&app.server.factory).await)
}

#[derive(Deserialize)]
struct Grant {
    stage: String,
    function: String,
    up: bool,
}

async fn capability_move(State(app): State<App>, Json(b): Json<Grant>) -> Response {
    let f = &app.server.factory;
    answer(api::govern::capability_move(f, b.stage, b.function, b.up).await)
}

async fn factory_capabilities(State(app): State<App>) -> Response {
    answer(api::govern::factory_capabilities(&app.server.factory).await)
}

async fn charters(State(app): State<App>) -> Response {
    answer(api::govern::charters(&app.server.factory, &app.server.machine).await)
}

async fn audit(State(app): State<App>, Query(page): Query<api::Page>) -> Response {
    answer(api::govern::audit(&app.server.factory, page).await)
}

async fn audit_export(State(app): State<App>) -> Response {
    let s = &app.server;
    answer(api::govern::audit_export(&s.data, &s.factory, &s.machine).await)
}

async fn settings(State(app): State<App>) -> Response {
    answer(api::settings::settings(&app.server.factory).await)
}

async fn settings_set(State(app): State<App>, Json(v): Json<SettingsView>) -> Response {
    let s = &app.server;
    answer(api::settings::settings_set(&s.events, &s.factory, v).await)
}

#[derive(Deserialize)]
struct Preset {
    preset: String,
}

async fn settings_preset(State(app): State<App>, Json(b): Json<Preset>) -> Response {
    let s = &app.server;
    answer(api::settings::settings_preset(&s.events, &s.factory, b.preset).await)
}

async fn settings_history(State(app): State<App>) -> Response {
    answer(api::settings::settings_history(&app.server.factory).await)
}

#[derive(Deserialize)]
struct Version {
    version: u64,
}

async fn settings_restore(State(app): State<App>, Json(b): Json<Version>) -> Response {
    let s = &app.server;
    answer(api::settings::settings_restore(&s.events, &s.factory, b.version).await)
}

async fn notify(State(app): State<App>) -> Response {
    let hint = app.server.webhook.hint();
    answer::<NotifyView>(Ok(NotifyView { webhook: hint }))
}

async fn notify_set(State(app): State<App>, Json(b): Json<NotifySet>) -> Response {
    answer(app.server.webhook.set(&b.webhook))
}

async fn notify_test(State(app): State<App>) -> Response {
    let text = "Bastion's notices reach this chat.";
    answer(app.server.webhook.send(text).await)
}

async fn models(State(app): State<App>) -> Response {
    answer(api::settings::models(&app.server.factory).await)
}

async fn accounts(State(app): State<App>) -> Response {
    answer(api::settings::accounts(&app.server.factory).await)
}

async fn issues(State(app): State<App>, Path(workspace): Path<String>) -> Response {
    let s = &app.server;
    answer(api::settings::issues(&s.factory, &s.machine, workspace).await)
}

async fn job_from_issue(
    State(app): State<App>,
    Extension(person): Extension<Person>,
    Path((workspace, number)): Path<(String, u64)>,
) -> Response {
    let s = &app.server;
    let from = FromIssue {
        workspace,
        number,
        requester: Some(person.name),
    };
    answer(api::settings::job_from_issue(&s.factory, &s.machine, from).await)
}

async fn people(State(app): State<App>) -> Response {
    answer(api::people::people(&app.server.factory).await)
}

async fn person_add(State(app): State<App>, Json(p): Json<Person>) -> Response {
    answer(api::people::add(&app.server.factory, p).await)
}

#[derive(Deserialize)]
struct NewRole {
    role: Role,
}

async fn person_role(
    State(app): State<App>,
    Path(name): Path<String>,
    Json(b): Json<NewRole>,
) -> Response {
    answer(api::people::set_role(&app.server.factory, name, b.role).await)
}

#[derive(Deserialize)]
struct Label {
    #[serde(default)]
    label: String,
}

async fn token_issue(
    State(app): State<App>,
    Path(name): Path<String>,
    Json(b): Json<Label>,
) -> Response {
    answer(api::people::issue(&app.server.factory, name, b.label).await)
}

async fn tokens(State(app): State<App>) -> Response {
    answer(api::people::tokens(&app.server.factory).await)
}

async fn token_revoke(State(app): State<App>, Path(id): Path<String>) -> Response {
    answer(api::people::revoke(&app.server.factory, id).await)
}

async fn providers(State(app): State<App>) -> Response {
    answer(api::providers::providers(&*app.server.engine).await)
}

async fn hosts(State(app): State<App>) -> Response {
    answer(api::hosts::hosts(&*app.server.engine).await)
}

async fn hosts_set(State(app): State<App>, Json(b): Json<Vec<HostView>>) -> Response {
    let set = api::hosts::hosts_set(&*app.server.engine, b).await;
    read_again(&app, set)
}

#[derive(Deserialize)]
struct Key {
    key: String,
}

async fn provider_key_set(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(b): Json<Key>,
) -> Response {
    let set = api::providers::key_set(&*app.server.engine, id, b.key).await;
    read_again(&app, set)
}

async fn provider_key_remove(State(app): State<App>, Path(id): Path<String>) -> Response {
    let removed = api::providers::key_remove(&*app.server.engine, id).await;
    read_again(&app, removed)
}

async fn codex_sign_in(State(app): State<App>) -> Response {
    answer(api::sign_in::codex_sign_in(&*app.server.engine).await)
}

#[derive(Deserialize)]
struct LoginId {
    login_id: String,
}

async fn codex_sign_in_poll(State(app): State<App>, Json(b): Json<LoginId>) -> Response {
    let polled = api::sign_in::codex_sign_in_poll(&*app.server.engine, b.login_id).await;
    if polled.as_deref() == Ok("ok") {
        app.server.events.publish(Change::Job { id: String::new() });
    }
    answer(polled)
}

async fn codex_sign_out(State(app): State<App>) -> Response {
    let out = api::sign_in::codex_sign_out(&*app.server.engine).await;
    read_again(&app, out)
}

/// After a key changes, every page reads again: the models offered follow
/// which providers have a key.
fn read_again(app: &App, done: Result<(), String>) -> Response {
    if done.is_ok() {
        app.server.events.publish(Change::Job { id: String::new() });
    }
    answer(done)
}

/// What the stream sends: a change with its number, or a request to read
/// everything again because changes were missed.
enum Out {
    Change(u64, Change),
    Resync,
}

/// The server's changes as server-sent events. A client that names the last
/// number it saw gets what it missed first, or a `resync` when the server no
/// longer holds it.
async fn events(
    State(app): State<App>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let bus = &app.server.events;
    // Subscribe before reading the backlog, so nothing falls between them.
    let live = bus.subscribe();
    let last = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let backlog: Vec<Out> = match last.map(|n| bus.since(n)) {
        None => Vec::new(),
        Some(Some(missed)) => missed.into_iter().map(|(n, c)| Out::Change(n, c)).collect(),
        Some(None) => vec![Out::Resync],
    };
    let sent = backlog
        .iter()
        .filter_map(|o| match o {
            Out::Change(n, _) => Some(*n),
            Out::Resync => None,
        })
        .max()
        .or(last);
    let live = stream::unfold(live, move |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok((n, _)) if sent.is_some_and(|s| n <= s) => continue,
                Ok((n, change)) => return Some((Out::Change(n, change), rx)),
                Err(RecvError::Lagged(_)) => return Some((Out::Resync, rx)),
                Err(RecvError::Closed) => return None,
            }
        }
    });
    let all = futures_util::StreamExt::map(
        futures_util::StreamExt::chain(stream::iter(backlog), live),
        |out| Ok(sse(out)),
    );
    Sse::new(all).keep_alive(KeepAlive::default())
}

fn sse(out: Out) -> Event {
    match out {
        Out::Change(n, change) => Event::default()
            .id(n.to_string())
            .data(serde_json::to_string(&change).unwrap_or_default()),
        Out::Resync => Event::default().event("resync").data("{}"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::body::Body;
    use axum::http::Request;
    use bastion_protocol::Link;
    use futures_util::StreamExt;
    use tower::ServiceExt;

    use super::*;
    use crate::engine::fake::FakeEngine;
    use crate::machine::fake::FakeMachine;
    use crate::stack::Stack;

    const TOKEN: &str = "t0ken";

    fn server() -> Arc<Server> {
        let stack = Stack {
            checkout: PathBuf::from("/checkout"),
            port: 49160,
            log: PathBuf::from("/log"),
        };
        let engine = Arc::new(FakeEngine::new(Link::Down));
        Server::new(engine, Arc::new(FakeMachine::new()), stack, "/data".into()).unwrap()
    }

    fn get(path: &str, token: Option<&str>) -> Request<Body> {
        let mut r = Request::get(path);
        if let Some(t) = token {
            r = r.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        r.body(Body::empty()).unwrap()
    }

    async fn call(server: Arc<Server>, request: Request<Body>) -> (StatusCode, String) {
        let response = router(server, TOKEN, "owner")
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    /// Every request a client can send reaches a handler here: no 404 for a
    /// path that does not exist, no 405 for the wrong method.
    #[test]
    fn every_request_a_client_sends_has_a_route() {
        crate::rt::block_on(async {
            for (method, route) in bastion_protocol::api::every() {
                let path = route.replace("{number}", "7").replace(['{', '}'], "");
                let body = match method {
                    "GET" | "DELETE" => Body::empty(),
                    _ => Body::from("{}"),
                };
                let request = Request::builder()
                    .method(method)
                    .uri(&path)
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(body)
                    .unwrap();
                let status = router(server(), TOKEN, "owner")
                    .oneshot(request)
                    .await
                    .unwrap()
                    .status();
                assert!(
                    status != StatusCode::NOT_FOUND && status != StatusCode::METHOD_NOT_ALLOWED,
                    "{method} {route} answered {status}"
                );
            }
        });
    }

    /// Fail unless `body`, as a client sends it, reads as `T`.
    fn reads<T: serde::de::DeserializeOwned>(body: impl Serialize) {
        let value = serde_json::to_value(body).unwrap();
        if let Err(e) = serde_json::from_value::<T>(value.clone()) {
            panic!(
                "{value} does not read as {}: {e}",
                std::any::type_name::<T>()
            );
        }
    }

    /// Each typed request's body on jobs and governance reads as the body
    /// its handler takes.
    #[test]
    fn every_typed_body_reads_as_its_handlers_body() {
        use bastion_protocol::api;
        let held = HeldCallRef {
            job: "j1".into(),
            call: "c1".into(),
        };
        let s = |v: &str| v.to_string();
        reads::<Submit>(api::JobSubmit {
            workspace: s("w"),
            request: s("r"),
            pipeline: Some(s("full_no_acceptance")),
            workflow: None,
            model: Some(s("deepseek/deepseek-flash")),
            ..Default::default()
        });
        reads::<Text>(api::JobComment {
            id: s("j1"),
            text: s("x"),
        });
        reads::<Parts>(api::JobParts {
            id: s("j1"),
            parts: vec![s("a")],
        });
        reads::<SetDial>(api::DialsSet {
            stage: s("plan"),
            dial: s("dark"),
        });
        reads::<Arm>(api::EvalApply {
            id: s("e1"),
            arm: s("a"),
        });
        reads::<RefuseCall>(api::HeldRefuse {
            held: held.clone(),
            feedback: s("no"),
        });
        reads::<HeldCallRef>(api::HeldAllow(held));
        reads::<Up>(api::ConstraintMove {
            id: s("c"),
            up: true,
        });
        reads::<Grant>(api::CapabilityMove {
            stage: s("build"),
            function: s("f"),
            up: false,
        });
        reads::<Preset>(api::SettingsPreset { preset: s("low") });
        reads::<Version>(api::SettingsRestore { version: 3 });
    }

    /// The same for the bodies of people, providers, workspaces and
    /// workflows.
    #[test]
    fn every_admin_body_reads_as_its_handlers_body() {
        use bastion_protocol::api;
        let s = |v: &str| v.to_string();
        reads::<NewRole>(api::PersonRole {
            name: s("ana"),
            role: s("member"),
        });
        reads::<Person>(api::PersonAdd(bastion_protocol::PersonView {
            name: s("ana"),
            role: s("viewer"),
        }));
        reads::<Label>(api::TokenIssue {
            name: s("ana"),
            label: s("laptop"),
        });
        reads::<Key>(api::ProviderKeySet {
            id: s("x"),
            key: s("k"),
        });
        reads::<crate::api::factory::WorkspaceSettings>(api::WorkspaceEdit {
            name: s("w"),
            branch: s("main"),
            checks: s(""),
            max_jobs: Some(2),
            revision_budget: Some(4),
            branch_prefix: s("ian/"),
            setup: s("make deps"),
            cleanup: s(""),
            env: s("CI=1"),
            gates: s("lint: ruff check {files}"),
            autofix: s(""),
            image: s("browser"),
        });
        reads::<crate::api::factory::DraftRequest>(api::WorkspaceDraft {
            path: s("/repo"),
            branch: s("main"),
        });
        reads::<LoginId>(api::CodexSignInPoll { login_id: s("l") });
        reads::<CopyWorkflow>(api::WorkflowCopy {
            from: s("a"),
            name: s("b"),
        });
    }

    /// A server whose factory runs on a fake store holding a viewer, a member
    /// with one job, and their tokens.
    async fn staffed() -> Arc<Server> {
        use crate::people_store::{NewToken, PeopleStore, hash};
        use crate::store::fake::FakeStore;
        use crate::store::{Record, Runtime, Store};
        let s = server();
        let store = Arc::new(FakeStore::default());
        let factory = crate::factory::Factory::new(
            s.engine.clone(),
            store.clone(),
            s.machine.clone(),
            Arc::new(|_: bastion_protocol::Change| {}),
            "/scratch".into(),
        );
        let _ = s.factory.set(factory);
        {
            for (name, role) in [("vic", Role::Viewer), ("mia", Role::Member)] {
                let p = Person {
                    name: name.into(),
                    role,
                };
                store.add_person(&p).await.unwrap();
                store
                    .add_token(NewToken {
                        person: name,
                        hash: &hash(&format!("{name}-token")),
                        label: "",
                        at: 1,
                    })
                    .await
                    .unwrap();
            }
            let record = Record {
                job: bastion_core::Job::new("j1".into(), "w".into(), "r".into(), 1),
                runtime: Runtime {
                    requester: Some("ana".into()),
                    ..Runtime::default()
                },
                rev: 1,
                created_at: 1,
            };
            store.insert(&record).await.unwrap();
        }
        s
    }

    fn post(path: &str, token: &str, body: &str) -> Request<Body> {
        Request::post(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[test]
    fn each_role_may_do_only_what_it_is_for() {
        crate::rt::block_on(async {
            let s = staffed().await;
            let (status, body) = call(s.clone(), get("/me", Some(TOKEN))).await;
            assert_eq!(status, StatusCode::OK);
            assert!(
                body.contains("\"admin\""),
                "the owner's token is the admin: {body}"
            );

            let (status, _) = call(s.clone(), get("/jobs", Some("vic-token"))).await;
            assert_eq!(status, StatusCode::OK, "a viewer reads");
            let (status, body) = call(
                s.clone(),
                post("/jobs", "vic-token", r#"{"workspace":"w","request":"r"}"#),
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert!(
                body.contains("vic is a Viewer, which may not submit a request"),
                "{body}"
            );

            let (status, _) = call(
                s.clone(),
                post("/settings/preset", "mia-token", r#"{"preset":"low"}"#),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "a member does not manage the server"
            );
            let (status, body) = call(s.clone(), post("/jobs/j1/approve", "mia-token", "")).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "j1 is ana's");
            assert!(body.contains("only ana or an admin"), "{body}");
            let (status, _) = call(s.clone(), post("/jobs/nope/approve", "mia-token", "")).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "no such job");

            let (status, _) = call(s, get("/jobs", Some("nobody"))).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
        });
    }

    #[test]
    fn a_failure_while_the_engine_is_down_is_unavailable_not_a_bad_request() {
        crate::rt::block_on(async {
            let s = staffed().await;
            let bad = post("/jobs", TOKEN, r#"{"workspace":"","request":""}"#);
            let (status, _) = call(s, bad).await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        });
    }

    #[test]
    fn the_audit_keeps_changes_and_refusals_but_not_reads() {
        let on_job = Act::OnJob { own: true };
        assert_eq!(audit_kind(on_job, StatusCode::OK), Some("act"));
        assert_eq!(
            audit_kind(Act::Read, StatusCode::FORBIDDEN),
            Some("refused")
        );
        assert_eq!(audit_kind(on_job, StatusCode::FORBIDDEN), Some("refused"));
        assert_eq!(audit_kind(Act::Read, StatusCode::OK), None);
        assert_eq!(audit_kind(Act::Manage, StatusCode::BAD_REQUEST), None);
    }

    #[test]
    fn a_request_without_the_token_is_refused() {
        crate::rt::block_on(async {
            let (status, _) = call(server(), get("/jobs", None)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            // Before the database opens, only the owner's token can be checked.
            let (status, _) = call(server(), get("/jobs", Some("wrong"))).await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        });
    }

    #[test]
    fn routes_answer_with_json_and_a_factory_not_started_is_unavailable() {
        crate::rt::block_on(async {
            let (status, body) = call(server(), get("/engine", Some(TOKEN))).await;
            assert_eq!(status, StatusCode::OK);
            assert!(body.contains("\"link\""), "{body}");
            let (status, body) = call(server(), get("/jobs", Some(TOKEN))).await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert!(body.contains(NOT_STARTED), "{body}");
        });
    }

    /// The first `count` chunks of an event stream, as text.
    async fn first_events(server: Arc<Server>, last: Option<&str>, count: usize) -> String {
        let mut request = get("/events", Some(TOKEN));
        if let Some(id) = last {
            request
                .headers_mut()
                .insert("last-event-id", id.parse().unwrap());
        }
        let response = router(server, TOKEN, "owner")
            .oneshot(request)
            .await
            .unwrap();
        let mut body = response.into_body().into_data_stream();
        let mut text = String::new();
        for _ in 0..count {
            let chunk = body.next().await.unwrap().unwrap();
            text.push_str(&String::from_utf8_lossy(&chunk));
        }
        text
    }

    #[test]
    fn events_replay_what_a_reconnect_missed_or_ask_for_a_resync() {
        crate::rt::block_on(async {
            let s = server();
            let mut seen = s.events.subscribe();
            s.events.publish(Change::Job { id: "j1".into() });
            s.events.publish(Change::Job { id: "j2".into() });
            // The engine watcher may publish first; find j1's number.
            let first = loop {
                if let (n, Change::Job { id }) = seen.recv().await.unwrap()
                    && id == "j1"
                {
                    break n;
                }
            };

            let replay = first_events(s.clone(), Some(&first.to_string()), 1).await;
            assert!(replay.contains(&format!("id: {}", first + 1)), "{replay}");
            assert!(replay.contains("\"id\":\"j2\""), "{replay}");
            assert!(!replay.contains("j1"), "{replay}");

            let stale = first_events(s.clone(), Some("1"), 1).await;
            assert!(stale.contains("event: resync"), "{stale}");
        });
    }

    #[test]
    fn events_stream_each_change_as_it_happens() {
        crate::rt::block_on(async {
            let s = server();
            let publisher = s.clone();
            crate::rt::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                publisher.events.publish(Change::Console);
            });
            // The engine watcher may publish its status first, so read on.
            let response = router(s, TOKEN, "owner")
                .oneshot(get("/events", Some(TOKEN)))
                .await
                .unwrap();
            let mut body = response.into_body().into_data_stream();
            let found = async {
                let mut text = String::new();
                while !text.contains("\"kind\":\"console\"") {
                    let chunk = body.next().await.unwrap().unwrap();
                    text.push_str(&String::from_utf8_lossy(&chunk));
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(2), found)
                .await
                .expect("the console change arrives");
        });
    }
}
