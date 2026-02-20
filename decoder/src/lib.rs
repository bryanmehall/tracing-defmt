use defmt_decoder::{DecodeError, Frame, Location, StreamDecoder, Table};
use std::collections::BTreeMap;
use tracing::{info, span, Level, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Defmt decode error: {0}")]
    Defmt(#[from] DecodeError),
    #[error("Elf parsing error: {0}")]
    Elf(String),
}

pub struct TraceDecoder {
    table: Table,
    locations: BTreeMap<u64, Location>,
}

impl TraceDecoder {
    pub fn new(elf_data: &[u8]) -> Result<Self, Error> {
        let table = Table::parse(elf_data)
            .map_err(|e| Error::Elf(format!("{:?}", e)))?
            .ok_or_else(|| Error::Elf("No defmt table found".to_string()))?;

        let locations = table
            .get_locations(elf_data)
            .map_err(|e| Error::Elf(format!("Locs: {:?}", e)))?;

        Ok(Self { table, locations })
    }

    pub fn new_stream(&self) -> TraceStream {
        let stream_decoder = self.table.new_stream_decoder();
        TraceStream {
            parent: self,
            stream_decoder: Some(stream_decoder),
            span_stack: Vec::new(),
        }
    }
}

pub struct TraceStream<'a> {
    parent: &'a TraceDecoder,
    stream_decoder: Option<Box<dyn StreamDecoder + 'a>>,
    span_stack: Vec<Span>,
}

impl<'a> TraceStream<'a> {
    pub fn process(&mut self, data: &[u8]) -> Result<(), Error> {
        let mut decoder = self.stream_decoder.take().unwrap();
        decoder.received(data);

        loop {
            match decoder.decode() {
                Ok(frame) => self.handle_frame(frame),
                Err(DecodeError::UnexpectedEof) => break,
                Err(DecodeError::Malformed) => {
                    eprintln!("⚠️  Defmt stream malformed. Resetting decoder...");
                    decoder = self.parent.table.new_stream_decoder();
                    break;
                }
            }
        }

        self.stream_decoder = Some(decoder);
        Ok(())
    }

    fn handle_frame(&mut self, frame: Frame) {
        let message = frame.display(false).to_string();

        if let Some(idx) = message.find("span_enter: ") {
            let rest = &message[idx + "span_enter: ".len()..];
            self.handle_span_enter(rest, &frame);
        } else if let Some(idx) = message.find("span_exit: ") {
            let rest = &message[idx + "span_exit: ".len()..];
            self.handle_span_exit(rest);
        } else {
            self.handle_log(&message, &frame);
        }
    }
    fn handle_span_enter(&mut self, name: &str, frame: &Frame) {
        let clean_name = if let Some(idx) = name.find("; file=") {
            &name[..idx]
        } else {
            name
        };

        let mut file = String::new();
        let mut line = 0i64;
        let mut module = String::from("rp_pico");

        if let Some(loc) = self.parent.locations.get(&frame.index()) {
            file = loc.file.display().to_string();
            line = loc.line as i64;
            module = loc.module.clone();
        }

        let parent_span = self.span_stack.last();

        // We use a static name "device_span" because tracing requires static names.
        // We set OTel semantic conventions via attributes.
        // tracing-opentelemetry might map "otel_name" field to span name, so we provide it.
        let span = if let Some(parent) = parent_span {
            span!(
                target: "device_log",
                parent: parent,
                Level::INFO,
                "device_span",
                otel_name = clean_name
            )
        } else {
            span!(
                target: "device_log",
                Level::INFO,
                "device_span",
                otel_name = clean_name
            )
        };

        // Set semantic conventions attributes
        span.set_attribute("otel.name", clean_name.to_string()); // Override span name
        span.set_attribute("code.function", clean_name.to_string());
        span.set_attribute("code.filepath", file);
        span.set_attribute("code.lineno", line);
        span.set_attribute("code.namespace", module);

        self.span_stack.push(span);
    }

    fn handle_span_exit(&mut self, _name: &str) {
        self.span_stack.pop();
    }

    fn handle_log(&mut self, message: &str, frame: &Frame) {
        let mut file = String::new();
        let mut line = 0i64;
        let mut module = String::from("rp_pico");

        if let Some(loc) = self.parent.locations.get(&frame.index()) {
            file = loc.file.display().to_string();
            line = loc.line as i64;
            module = loc.module.clone();
        }

        let parent_span = self.span_stack.last();

        // Use underscores for tracing fields, but OTel layer might NOT map these to dots automatically.
        // However, we cannot use dots in info! macro.
        if let Some(span) = parent_span {
            info!(
                target: "device_log",
                parent: span,
                code_filepath = file.as_str(),
                code_lineno = line,
                code_namespace = module.as_str(),
                "{}",
                message
            );
        } else {
            info!(
                target: "device_log",
                code_filepath = file.as_str(),
                code_lineno = line,
                code_namespace = module.as_str(),
                "{}",
                message
            );
        }

        eprintln!("{}", message);
    }
}
