//! Stderr subscriber. Only compiled for binaries (`feature = "subscriber"`).

use std::fmt;
use std::time::Instant;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{prelude::*, EnvFilter, Registry};

use crate::{
    banner, eprint_event, eprint_span, filter_directive, mark_main_thread, mark_origin, LaneDesc,
};

/// One process-wide install. Safe to call twice (second is a no-op).
pub fn init(config: Init<'_>) {
    mark_main_thread();
    mark_origin();
    let filter = EnvFilter::try_new(filter_directive()).unwrap_or_else(|_| EnvFilter::new("off"));
    let _ = Registry::default()
        .with(filter)
        .with(LfLayer {
            verbose: crate::verbose(),
        })
        .try_init();
    let lanes: Vec<LaneDesc> = config
        .lanes
        .iter()
        .map(|lane| LaneDesc {
            kind: lane.kind,
            detail: lane.detail,
        })
        .collect();
    banner(config.app, &lanes);
}

/// Binary identity + the lanes printed at startup.
#[derive(Debug, Clone, Copy)]
pub struct Init<'a> {
    /// `localgallery`, `localmusic`, `localcontacts`, …
    pub app: &'a str,
    /// Foreground / background / idle lines for this binary.
    pub lanes: &'a [Lane],
}

/// One line of the startup lane map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lane {
    /// `FOREGROUND`, `BACKGROUND`, or `IDLE`.
    pub kind: &'static str,
    /// What runs on that lane.
    pub detail: &'static str,
}

struct LfLayer {
    verbose: bool,
}

#[derive(Clone)]
struct SpanData {
    start: Instant,
    area: String,
    label: String,
    extras: String,
    always: bool,
}

#[derive(Default)]
struct Fields {
    area: String,
    label: String,
    message: String,
    extras: String,
    always: bool,
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record(field.name(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.record(field.name(), value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        if field.name() == "always" {
            self.always = value;
        } else {
            self.record(field.name(), value.to_string());
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record(field.name(), value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record(field.name(), value.to_string());
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.record(field.name(), value.to_string());
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.record(field.name(), value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.record(field.name(), value.to_string());
    }
}

impl Fields {
    fn record(&mut self, name: &str, value: String) {
        let value = strip_debug_quotes(&value);
        match name {
            "area" => self.area = value,
            "label" => self.label = value,
            "message" => self.message = value,
            "extras" => self.extras = value,
            "always" => {}
            _ => {
                if !self.extras.is_empty() {
                    self.extras.push(' ');
                }
                self.extras.push_str(name);
                self.extras.push('=');
                self.extras.push_str(&value);
            }
        }
    }
}

fn strip_debug_quotes(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

impl<S> Layer<S> for LfLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanData {
                start: Instant::now(),
                area: fields.area,
                label: fields.label,
                extras: fields.extras,
                always: fields.always,
            });
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut fields = Fields::default();
        values.record(&mut fields);
        let mut extensions = span.extensions_mut();
        if let Some(data) = extensions.get_mut::<SpanData>() {
            if !fields.area.is_empty() {
                data.area = fields.area;
            }
            if !fields.label.is_empty() {
                data.label = fields.label;
            }
            if !fields.extras.is_empty() {
                data.extras = fields.extras;
            }
            data.always |= fields.always;
        }
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() != "lf" && !event.metadata().target().starts_with("lf.") {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let area = if fields.area.is_empty() {
            event.metadata().target().trim_start_matches("lf.")
        } else {
            fields.area.as_str()
        };
        eprint_event(area, &fields.message);
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let Some(data) = span.extensions().get::<SpanData>().cloned() else {
            return;
        };
        let ms = data.start.elapsed().as_secs_f64() * 1000.0;
        if !data.always && !self.verbose && ms < 5.0 {
            return;
        }
        let area = if data.area.is_empty() {
            "span"
        } else {
            data.area.as_str()
        };
        eprint_span(area, &data.label, &data.extras, ms);
    }
}
