use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    Expr, ExprLit, FnArg, Ident, ItemFn, Lit, LitStr, Meta, Pat, Token,
};

// =============================================================================
// #[instrument]
// =============================================================================

/// Instruments a function to create a span when called.
///
/// This is a facade for `tracing::instrument`.
///
/// # Arguments
/// * `level` - The log level to use. Defaults to info.
/// * `name` - Sets the name of the span. Defaults to the function name.
/// * `skip` - A list of arguments to skip logging.
///
/// # Example
/// ```rust
/// #[instrument(level = "debug", skip(y))]
/// fn my_fn(x: u32, y: u32) { ... }
/// ```
#[proc_macro_attribute]
pub fn instrument(args: TokenStream, item: TokenStream) -> TokenStream {
    let args_parsed = parse_macro_input!(args with Punctuated::<Meta, Token![,]>::parse_terminated);
    let item_fn = parse_macro_input!(item as ItemFn);

    let fn_name_ident = item_fn.sig.ident.clone();
    let fn_name_str = fn_name_ident.to_string();

    let mut level = "info".to_string();
    let mut name = fn_name_str.clone();
    let mut skip = Vec::new();

    // Parse attributes
    for meta in args_parsed {
        match meta {
            Meta::NameValue(nv) => {
                if nv.path.is_ident("level") {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(lit), ..
                    }) = nv.value
                    {
                        level = lit.value();
                    }
                } else if nv.path.is_ident("name") {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(lit), ..
                    }) = nv.value
                    {
                        name = lit.value();
                    }
                }
            }
            Meta::List(list) => {
                if list.path.is_ident("skip") {
                    let nested_ids = list
                        .parse_args_with(Punctuated::<Ident, Token![,]>::parse_terminated)
                        .unwrap_or_default();
                    for id in nested_ids {
                        skip.push(id.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    let macro_path = level_to_macro_path(&level);

    // Build format string and arguments
    // We prefix with "span_enter: " to make it easily parsable for host tools
    let mut fmt_str = String::from("span_enter: ");
    fmt_str.push_str(&name);

    let mut log_args = Vec::new();
    let mut first = true;
    let mut has_args = false;

    for input in &item_fn.sig.inputs {
        if let FnArg::Typed(pat_type) = input {
            if let Pat::Ident(pat_ident) = &*pat_type.pat {
                let arg_name = pat_ident.ident.to_string();
                if !skip.contains(&arg_name) {
                    if first {
                        fmt_str.push_str("(");
                        first = false;
                    } else {
                        fmt_str.push_str(", ");
                    }
                    fmt_str.push_str(&arg_name);
                    fmt_str.push_str("={}");
                    let ident = &pat_ident.ident;
                    log_args.push(quote!(#ident));
                    has_args = true;
                }
            }
        }
    }

    if has_args {
        fmt_str.push(')');
    }

    let block = &item_fn.block;
    let attrs = &item_fn.attrs;
    let vis = &item_fn.vis;
    let sig = &item_fn.sig;

    let expanded = quote! {
        #(#attrs)*
        #vis #sig {
            #macro_path!(#fmt_str, #(#log_args),*);
            struct DefmtInstrumentGuard;
            impl Drop for DefmtInstrumentGuard {
                fn drop(&mut self) {
                    // We emit "span_exit: name" to allow matching exit events
                    #macro_path!("span_exit: {}", #name);
                }
            }
            let _guard = DefmtInstrumentGuard;
            #block
        }
    };

    TokenStream::from(expanded)
}

// =============================================================================
// Log Macros
// =============================================================================

struct LogArgs {
    fields: Vec<(String, Expr)>,
    fmt_str: Option<LitStr>,
    fmt_args: Vec<Expr>,
}

impl Parse for LogArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut fields = Vec::new();
        let mut fmt_str = None;
        let mut fmt_args = Vec::new();
        let mut has_fmt_str = false;

        while !input.is_empty() {
            // 1. Check for key-value: key = value
            if input.peek(Ident) && input.peek2(Token![=]) {
                let key: Ident = input.parse()?;
                let _eq: Token![=] = input.parse()?;
                let val: Expr = input.parse()?;
                fields.push((key.to_string(), val));

                if input.peek(Token![,]) {
                    let _ = input.parse::<Token![,]>();
                }
                continue;
            }

            // 2. Check for target: target: value (ignore)
            if input.peek(Ident) && input.peek2(Token![:]) {
                let _key: Ident = input.parse()?;
                let _colon: Token![:] = input.parse()?;
                let _val: Expr = input.parse()?;
                if input.peek(Token![,]) {
                    let _ = input.parse::<Token![,]>();
                }
                continue;
            }

            // 3. Check for Format String (LitStr)
            if !has_fmt_str && input.peek(LitStr) {
                fmt_str = Some(input.parse()?);
                has_fmt_str = true;
                if input.peek(Token![,]) {
                    let _ = input.parse::<Token![,]>();
                }
                continue;
            }

            // 4. Expression
            let expr: Expr = input.parse()?;
            if has_fmt_str {
                // If we have a format string, this is a format argument
                fmt_args.push(expr);
            } else {
                // If we don't have a format string, this is a shorthand field
                // `x` -> `x = x`
                if let Expr::Path(ep) = &expr {
                    if let Some(ident) = ep.path.get_ident() {
                        fields.push((ident.to_string(), expr.clone()));
                    } else {
                        // Ignore complex expressions that aren't fields or fmt args
                    }
                }
            }

            if input.peek(Token![,]) {
                let _ = input.parse::<Token![,]>();
            }
        }

        Ok(LogArgs {
            fields,
            fmt_str,
            fmt_args,
        })
    }
}

fn impl_log_macro(level: &str, args: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as LogArgs);
    let macro_path = level_to_macro_path(level);

    let mut final_fmt_str = if let Some(fs) = args.fmt_str {
        fs.value()
    } else {
        String::new()
    };

    let mut final_args = args.fmt_args;

    // Append fields to format string
    // defmt doesn't support structured fields disjoint from the message.
    // We append them: "msg, key={}, key2={}"
    let mut first = true;
    for (key, val) in args.fields {
        if first {
            if !final_fmt_str.is_empty() {
                final_fmt_str.push_str(", ");
            }
            first = false;
        } else {
            final_fmt_str.push_str(", ");
        }
        final_fmt_str.push_str(&key);
        final_fmt_str.push_str("={}");
        final_args.push(val);
    }

    quote! {
        #macro_path!(#final_fmt_str, #(#final_args),*)
    }
    .into()
}

#[proc_macro]
pub fn trace(input: TokenStream) -> TokenStream {
    impl_log_macro("trace", input)
}

#[proc_macro]
pub fn debug(input: TokenStream) -> TokenStream {
    impl_log_macro("debug", input)
}

#[proc_macro]
pub fn info(input: TokenStream) -> TokenStream {
    impl_log_macro("info", input)
}

#[proc_macro]
pub fn warn(input: TokenStream) -> TokenStream {
    impl_log_macro("warn", input)
}

#[proc_macro]
pub fn error(input: TokenStream) -> TokenStream {
    impl_log_macro("error", input)
}

// =============================================================================
// Helpers
// =============================================================================

fn level_to_macro_path(level: &str) -> proc_macro2::TokenStream {
    match level {
        "trace" => quote!(defmt::trace),
        "debug" => quote!(defmt::debug),
        "info" => quote!(defmt::info),
        "warn" => quote!(defmt::warn),
        "error" => quote!(defmt::error),
        _ => quote!(defmt::info),
    }
}
