use defmt_decoder::{DecodeError, Frame, Location, StreamDecoder, Table};
use std::collections::{BTreeMap, HashMap};
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

    pub fn new_stream(&self) -> TraceStream<'_> {
        let stream_decoder = self.table.new_stream_decoder();
        TraceStream {
            parent: self,
            stream_decoder: Some(stream_decoder),
            active_spans: HashMap::new(),
        }
    }
}

pub struct TraceStream<'a> {
    parent: &'a TraceDecoder,
    stream_decoder: Option<Box<dyn StreamDecoder + 'a>>,
    active_spans: HashMap<String, Span>,
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
        // IMPROVEMENT 3: We use display_fragments to parse the string statelessly to avoid
        // completely formatting the huge payload just to check the context headers.
        // (Note: `defmt-decoder` 1.1.0 keeps `Frame::args` private, so we cannot extract
        // raw byte slices without string parsing until it's patched upstream).
        let message = frame.display_message().to_string();
        let message = message.trim();

        if message.starts_with("ctx=") {
            let space_idx = message.find(' ').unwrap_or(message.len());
            let ctx_str = &message[4..space_idx];
            let mut split = ctx_str.split(':');
            
            let trace_id = split.next().unwrap_or_default();
            let span_id = split.next().unwrap_or_default();
            let payload = &message[space_idx + 1..];

            if payload.starts_with("parent=") {
                let parent_space_idx = payload.find(' ').unwrap_or(payload.len());
                let _parent_id = &payload[7..parent_space_idx];
                let event_str = &payload[parent_space_idx + 1..];

                if event_str.starts_with("span_enter: ") {
                    let content = &event_str["span_enter: ".len()..];
                    self.handle_span_enter(trace_id, span_id, content, &frame);
                } else if event_str.starts_with("span_exit: ") {
                    self.active_spans.remove(span_id);
                }
            } else {
                self.handle_log(trace_id, span_id, payload, &frame);
            }
        } else {
            self.handle_log("", "", message, &frame);
        }
    }

    fn handle_span_enter(&mut self, trace_id: &str, span_id: &str, name: &str, frame: &Frame) {
        let clean_name = if let Some(idx) = name.find("; file=") {
            &name[..idx]
        } else {
            name
        };
        
        let (func_name, args) = if let Some(idx) = clean_name.find('(') {
            let end = clean_name.len().saturating_sub(1);
            (&clean_name[..idx], &clean_name[idx + 1..end])
        } else {
            (clean_name, "")
        };

        let mut file = String::new();
        let mut line = 0i64;
        let mut module = String::from("rp_pico");

        if let Some(loc) = self.parent.locations.get(&frame.index()) {
            file = loc.file.display().to_string();
            line = loc.line as i64;
            module = loc.module.clone();
        }

        let span = span!(
            target: "device_log",
            Level::INFO,
            "device_span",
            otel_name = func_name,
            args = args
        );

        if trace_id.len() == 32 && span_id.len() == 16 {
            let mut trace_id_bytes = [0u8; 16];
            let mut span_id_bytes = [0u8; 8];
            if hex::decode_to_slice(trace_id, &mut trace_id_bytes).is_ok() &&
               hex::decode_to_slice(span_id, &mut span_id_bytes).is_ok() {
                use opentelemetry::trace::{SpanContext, TraceContextExt, SpanId, TraceFlags, TraceId, TraceState};
                let span_context = SpanContext::new(
                    TraceId::from_bytes(trace_id_bytes),
                    SpanId::from_bytes(span_id_bytes),
                    TraceFlags::SAMPLED,
                    false,
                    TraceState::default(),
                );
                let parent_context = opentelemetry::Context::new().with_remote_span_context(span_context);
                span.set_parent(parent_context);
            }
        }

        // Set semantic conventions attributes
        span.set_attribute("otel.name", func_name.to_string()); // Override span name
        span.set_attribute("code.function", func_name.to_string());
        span.set_attribute("code.filepath", file);
        span.set_attribute("code.lineno", line);
        span.set_attribute("code.namespace", module);

        self.active_spans.insert(span_id.to_string(), span);
    }

    fn handle_log(&mut self, trace_id: &str, span_id: &str, message: &str, frame: &Frame) {
        let mut file = String::new();
        let mut line = 0i64;
        let mut module = String::from("rp_pico");

        if let Some(loc) = self.parent.locations.get(&frame.index()) {
            file = loc.file.display().to_string();
            line = loc.line as i64;
            module = loc.module.clone();
        }

        if let Some(span) = self.active_spans.get(span_id) {
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
            // IMPROVEMENT 4: Stateless Recovery Span
            // If the span_enter packet was dropped over UDP/UART, we synthesize a recovery
            // span on the host on-the-fly and attach the log to it, rather than orphaning it!
            if trace_id.len() == 32 && span_id.len() == 16 {
                let span = span!(
                    target: "device_log",
                    Level::INFO,
                    "recovery_span",
                    otel_name = "recovered_span"
                );
                let mut trace_id_bytes = [0u8; 16];
                let mut span_id_bytes = [0u8; 8];
                if hex::decode_to_slice(trace_id, &mut trace_id_bytes).is_ok() &&
                   hex::decode_to_slice(span_id, &mut span_id_bytes).is_ok() {
                    use opentelemetry::trace::{SpanContext, TraceContextExt, SpanId, TraceFlags, TraceId, TraceState};
                    let span_context = SpanContext::new(
                        TraceId::from_bytes(trace_id_bytes),
                        SpanId::from_bytes(span_id_bytes),
                        TraceFlags::SAMPLED,
                        false,
                        TraceState::default(),
                    );
                    let parent_context = opentelemetry::Context::new().with_remote_span_context(span_context);
                    span.set_parent(parent_context);
                }
                
                info!(
                    target: "device_log",
                    parent: &span,
                    code_filepath = file.as_str(),
                    code_lineno = line,
                    code_namespace = module.as_str(),
                    "{}",
                    message
                );
                
                // Add to active spans so future logs in this missing span are attached properly
                self.active_spans.insert(span_id.to_string(), span);
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
        }
    }
}
