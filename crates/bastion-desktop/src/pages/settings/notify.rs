//! Notifications (F9): the chat webhook a Bastion server posts each notice
//! to. The address is a credential, so the page shows only its hint.

use bastion_protocol::api::{self, Request};
use bastion_protocol::{NotifySet, NotifyView};
use iced::widget::{column, row, text, text_input};
use iced::{Element, Task};

use super::{card, small};
use crate::app::Message;
use crate::pages::Ctx;
use crate::theme;

#[derive(Default)]
pub struct State {
    view: NotifyView,
    typed: String,
    note: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Result<NotifyView, String>),
    Type(String),
    Save,
    Forget,
    Test,
    Done(Result<(), String>),
    Tested(Result<(), String>),
}

fn wrap(msg: Msg) -> Message {
    Message::Settings(super::Msg::Notify(msg))
}

fn task<R>(ctx: &Ctx, request: R, then: fn(Result<R::Answer, String>) -> Msg) -> Task<Message>
where
    R: Request + Send + Sync + 'static,
    R::Answer: Send + 'static,
{
    ctx.task(&ctx.current, request, then).map(wrap)
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    task(ctx, api::Notify, Msg::Read)
}

fn set(ctx: &Ctx, webhook: String) -> Task<Message> {
    task(ctx, api::NotifySetWebhook(NotifySet { webhook }), Msg::Done)
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Read(Ok(view)) => state.view = view,
        Msg::Type(t) => state.typed = t,
        Msg::Save => return set(ctx, state.typed.trim().to_string()),
        Msg::Forget => return set(ctx, String::new()),
        Msg::Test => return task(ctx, api::NotifyTest, Msg::Tested),
        Msg::Done(Ok(())) => {
            (state.typed, state.note) = Default::default();
            return load(ctx);
        }
        Msg::Tested(Ok(())) => state.note = "Sent. Look for it in the chat.".into(),
        Msg::Read(Err(e)) | Msg::Done(Err(e)) | Msg::Tested(Err(e)) => state.note = e,
    }
    Task::none()
}

pub fn view(state: &State) -> Element<'_, Message> {
    let now = match &state.view.webhook {
        Some(hint) => format!("Posting to {hint}"),
        None => "No webhook: notices reach the desktop and `bastion follow` only.".into(),
    };
    let field = text_input("https://hooks.slack.com/services/…", &state.typed)
        .on_input(|t| wrap(Msg::Type(t)))
        .on_submit(wrap(Msg::Save))
        .style(theme::field);
    let typed = !state.typed.trim().is_empty();
    let set = state.view.webhook.is_some();
    let buttons = row![
        small("Save", typed.then_some(wrap(Msg::Save))),
        small("Send a test", set.then_some(wrap(Msg::Test))),
        small("Forget it", set.then_some(wrap(Msg::Forget))),
    ]
    .spacing(8);
    let lede =
        "A chat webhook each notice is posted to: a job that waits on a person, fails or lands.";
    card(
        "Notifications",
        column![
            text(lede).size(13).color(theme::MUTED),
            text(now).size(13),
            field,
            buttons,
            text(&state.note).size(13).color(theme::MUTED),
        ],
    )
}
