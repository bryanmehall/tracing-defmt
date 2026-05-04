use defmt_decoder::{DecodeError, Frame, Location, StreamDecoder, Table};
use std::collections::{BTreeMap, HashMap};
use tracing::{info, span, Level, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use opentelemetry::trace::{SpanContext, SpanId, TraceFlags, TraceId, TraceState, TraceContextExt};

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
        let fragments = frame.fragments();
        let mut display_fragments = frame.display_fragments();

        if let Some(defmt_parser::Fragment::Literal(lit)) = fragments.first() {
            if lit == "ctx=" {
                let _ctx_lit = display_fragments.next(); // "ctx="
                let trace_id_str = display_fragments.next().unwrap_or_default(); // trace_id
                let _colon = display_fragments.next(); // ":"
                let span_id_str = display_fragments.next().unwrap_or_default(); // span_id
                let parent_lit = display_fragments.next().unwrap_or_default(); // " parent="
                
                if parent_lit == " parent=" {
                    let _parent_span_id_str = display_fragments.next().unwrap_or_default(); // parent_span_id
                    let event_type_lit = display_fragments.next().unwrap_or_default(); // " span_enter: " or " span_exit: " or " "

                    let trace_id_bytes: Vec<u8> = trace_id_str
                        .trim_start_matches('[')
                        .trim_end_matches(']')
                        .split(',')
                        .filter_map(|s| s.trim().parse::<u8>().ok())
                        .collect();

                    let span_id_bytes: Vec<u8> = span_id_str
                        .trim_start_matches('[')
                        .trim_end_matches(']')
                        .split(',')
                        .filter_map(|s| s.trim().parse::<u8>().ok())
                        .collect();

                    let trace_id_hex = hex::encode(&trace_id_bytes);
                    let span_id_hex = hex::encode(&span_id_bytes);

                    if event_type_lit == " span_enter: " {
                        let content = display_fragments.next().unwrap_or_default();
                        self.handle_span_enter(&trace_id_hex, &span_id_hex, &content, &frame);
                    } else if event_type_lit == " span_exit: " {
                        self.active_spans.remove(&span_id_hex);
                    } else {
                        // It's a normal log!
                        let mut payload = String::new();
                        while let Some(frag) = display_fragments.next() {
                            payload.push_str(&frag);
                        }
                        self.handle_log(&span_id_hex, &payload, &frame);
                    }
                }
                return;
            }
        }

        // Normal defmt log without ctx
        let message = frame.display_message().to_string();
        self.handle_log("", &message, &frame);
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

        span.set_attribute("otel.name", func_name.to_string()); 
        span.set_attribute("code.function", func_name.to_string());
        span.set_attribute("code.filepath", file);
        span.set_attribute("code.lineno", line);
        span.set_attribute("code.namespace", module);

        self.active_spans.insert(span_id.to_string(), span);
    }

    fn handle_log(&self, span_id: &str, message: &str, frame: &Frame) {
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
