//! See [Telemetry in Zed](https://zed.dev/docs/telemetry) for additional information.
//!
//! Zepi cut: no telemetry leaves the process. `event!` sites stay so upstream code
//! compiles unchanged, but nothing is queued and no client ever sees an event.
use futures::channel::mpsc;
pub use serde_json;
pub use telemetry_events::FlexibleEvent as Event;

/// Macro to create telemetry events and send them to the telemetry queue.
///
/// By convention, the name should be "Noun Verbed", e.g. "Keymap Changed"
/// or "Project Diagnostics Opened".
///
/// The properties can be any value that implements serde::Serialize.
///
/// ```
/// # let url = "https://example.com";
/// telemetry::event!("Keymap Changed", version = "1.0.0");
/// telemetry::event!("Documentation Viewed", url, source = "Extension Upsell");
/// ```
///
/// If you want to debug logging in development, export `RUST_LOG=telemetry=trace`
#[macro_export]
macro_rules! event {
    ($name:expr) => {{
        let event = $crate::Event {
            event_type: $name.to_string(),
            event_properties: std::collections::HashMap::new(),
        };
        $crate::send_event(event);
    }};
    ($name:expr, $($key:ident $(= $value:expr)?),+ $(,)?) => {{
        let event = $crate::Event {
            event_type: $name.to_string(),
            event_properties: std::collections::HashMap::from([
                $(
                    (stringify!($key).to_string(),
                        $crate::serde_json::value::to_value(&$crate::serialize_property!($key $(= $value)?))
                            .unwrap_or_else(|_| $crate::serde_json::to_value(&()).unwrap())
                    ),
                )+
            ]),
        };
        $crate::send_event(event);
    }};
}

#[macro_export]
macro_rules! serialize_property {
    ($key:ident) => {
        $key
    };
    ($key:ident = $value:expr) => {
        $value
    };
}

/// Drops the event. Kept for upstream call sites.
pub fn send_event(_event: Event) {}

/// No queue is installed, so `send_event` has nowhere to deliver.
pub fn init(_tx: mpsc::UnboundedSender<Event>) {}
