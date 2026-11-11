//! Each Bastion server's pushes (DECISIONS.md D29) as a subscription: one
//! stream per server, read by the client library.

use bastion_client::events;
pub use bastion_client::events::Feed;
use bastion_protocol::Server;
use futures_util::Stream;
use iced::Subscription;

/// The pushes of every server in `servers`, each tagged with its name.
pub fn follow(servers: &[Server]) -> Subscription<(String, Feed)> {
    let each = servers.iter().map(|s| {
        let key = (s.name.clone(), s.url.clone(), s.token.clone());
        Subscription::run_with(key, stream)
    });
    Subscription::batch(each)
}

type Key = (String, String, String);

fn stream(key: &Key) -> impl Stream<Item = (String, Feed)> + use<> {
    let (name, url, token) = key.clone();
    let server = Server { name, url, token };
    iced::stream::channel(64, async move |out| events::follow(server, out).await)
}
