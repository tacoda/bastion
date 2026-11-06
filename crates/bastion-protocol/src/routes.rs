//! Each server command's HTTP method and path (D28). The page sends a call
//! by this table, and the server's tests check every row has a route.

use crate::command;

/// Where a request's body comes from: nothing, one argument, or every
/// argument the path did not use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Body {
    Empty,
    Field(&'static str),
    Rest,
}

/// Each server command's method and path. `{name}` takes the argument of
/// that name. A command missing here is the window's own.
pub fn route(cmd: &str) -> Option<(&'static str, &'static str, Body)> {
    use crate::command;
    use Body::*;
    Some(match cmd {
        command::ENGINE_STATUS => ("GET", "/engine", Empty),
        command::DOCTOR => ("GET", "/doctor", Empty),
        command::STACK_STATUS => ("GET", "/stack", Empty),
        command::STACK_START => ("POST", "/stack/start", Empty),
        command::STACK_STOP => ("POST", "/stack/stop", Empty),
        command::CONSOLE => ("GET", "/console", Empty),
        command::CONTAINER_LOGS => ("POST", "/console/logs", Field("request")),
        command::CONTAINER_STOP => ("POST", "/console/{name}/stop", Empty),
        command::CONTAINER_START => ("POST", "/console/{name}/start", Empty),
        command::WORKSPACES => ("GET", "/workspaces", Empty),
        command::WORKSPACE_ADD => ("POST", "/workspaces", Field("workspace")),
        command::JOBS => ("GET", "/jobs", Empty),
        command::JOB => ("GET", "/jobs/{id}", Empty),
        command::JOB_SUBMIT => ("POST", "/jobs", Rest),
        command::JOB_APPROVE => ("POST", "/jobs/{id}/approve", Empty),
        command::JOB_COMMENT => ("POST", "/jobs/{id}/comment", Rest),
        command::JOB_STOP => ("POST", "/jobs/{id}/stop", Empty),
        command::DIALS => ("GET", "/dials", Empty),
        command::DIALS_SET => ("PUT", "/dials/{stage}", Rest),
        command::HELD_ALLOW => ("POST", "/held/allow", Field("held")),
        command::HELD_REFUSE => ("POST", "/held/refuse", Rest),
        command::CONSTRAINTS => ("GET", "/constraints", Empty),
        command::CONSTRAINT_ADD => ("POST", "/constraints", Field("constraint")),
        command::CONSTRAINT_MOVE => ("POST", "/constraints/{id}/move", Rest),
        command::CAPABILITIES => ("GET", "/capabilities", Empty),
        command::CAPABILITY_MOVE => ("POST", "/capabilities/move", Rest),
        command::FACTORY_CAPABILITIES => ("GET", "/capabilities/factory", Empty),
        command::CHARTERS => ("GET", "/charters", Empty),
        command::AUDIT => ("GET", "/audit", Empty),
        command::AUDIT_EXPORT => ("POST", "/audit/export", Empty),
        command::EVALS => ("GET", "/evals", Empty),
        command::EVAL_CREATE => ("POST", "/evals", Field("eval")),
        command::EVAL => ("GET", "/evals/{id}", Empty),
        command::EVAL_APPLY => ("POST", "/evals/{id}/apply", Rest),
        command::SETTINGS => ("GET", "/settings", Empty),
        command::SETTINGS_SET => ("PUT", "/settings", Field("settings")),
        command::SETTINGS_PRESET => ("POST", "/settings/preset", Rest),
        command::SETTINGS_HISTORY => ("GET", "/settings/history", Empty),
        command::SETTINGS_RESTORE => ("POST", "/settings/restore", Rest),
        command::MODELS => ("GET", "/models", Empty),
        command::ACCOUNTS => ("GET", "/accounts", Empty),
        command::ISSUES => ("GET", "/issues/{workspace}", Empty),
        command::JOB_FROM_ISSUE => ("POST", "/issues/{workspace}/{number}/job", Empty),
        command::ME => ("GET", "/me", Empty),
        command::PEOPLE => ("GET", "/people", Empty),
        command::PERSON_ADD => ("POST", "/people", Field("person")),
        command::PERSON_ROLE => ("PUT", "/people/{name}/role", Rest),
        command::TOKEN_ISSUE => ("POST", "/people/{name}/tokens", Rest),
        command::TOKENS => ("GET", "/tokens", Empty),
        command::TOKEN_REVOKE => ("DELETE", "/tokens/{id}", Empty),
        command::PROVIDERS => ("GET", "/providers", Empty),
        command::PROVIDER_KEY_SET => ("PUT", "/providers/{id}/key", Rest),
        command::PROVIDER_KEY_REMOVE => ("DELETE", "/providers/{id}/key", Empty),
        command::CODEX_SIGN_IN => ("POST", "/providers/openai-codex/sign-in", Empty),
        command::CODEX_SIGN_IN_POLL => ("POST", "/providers/openai-codex/sign-in/poll", Rest),
        command::CODEX_SIGN_OUT => ("POST", "/providers/openai-codex/sign-out", Empty),
        _ => return None,
    })
}

/// Every command this table routes.
pub const ROUTED: [&str; 56] = [
    command::ENGINE_STATUS,
    command::DOCTOR,
    command::STACK_STATUS,
    command::STACK_START,
    command::STACK_STOP,
    command::CONSOLE,
    command::CONTAINER_LOGS,
    command::CONTAINER_STOP,
    command::CONTAINER_START,
    command::WORKSPACES,
    command::WORKSPACE_ADD,
    command::JOBS,
    command::JOB,
    command::JOB_SUBMIT,
    command::JOB_APPROVE,
    command::JOB_COMMENT,
    command::JOB_STOP,
    command::DIALS,
    command::DIALS_SET,
    command::HELD_ALLOW,
    command::HELD_REFUSE,
    command::CONSTRAINTS,
    command::CONSTRAINT_ADD,
    command::CONSTRAINT_MOVE,
    command::CAPABILITIES,
    command::CAPABILITY_MOVE,
    command::FACTORY_CAPABILITIES,
    command::CHARTERS,
    command::AUDIT,
    command::AUDIT_EXPORT,
    command::EVALS,
    command::EVAL_CREATE,
    command::EVAL,
    command::EVAL_APPLY,
    command::SETTINGS,
    command::SETTINGS_SET,
    command::SETTINGS_PRESET,
    command::SETTINGS_HISTORY,
    command::SETTINGS_RESTORE,
    command::MODELS,
    command::ACCOUNTS,
    command::ISSUES,
    command::JOB_FROM_ISSUE,
    command::ME,
    command::PEOPLE,
    command::PERSON_ADD,
    command::PERSON_ROLE,
    command::TOKEN_ISSUE,
    command::TOKENS,
    command::TOKEN_REVOKE,
    command::PROVIDERS,
    command::PROVIDER_KEY_SET,
    command::PROVIDER_KEY_REMOVE,
    command::CODEX_SIGN_IN,
    command::CODEX_SIGN_IN_POLL,
    command::CODEX_SIGN_OUT,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_routed_command_has_a_route_and_the_window_commands_none() {
        for cmd in ROUTED {
            assert!(route(cmd).is_some(), "{cmd}");
        }
        for cmd in [
            command::QUIT,
            command::SERVERS,
            command::SERVER_ADD,
            command::SERVER_REMOVE,
        ] {
            assert!(route(cmd).is_none(), "{cmd}");
        }
    }
}
