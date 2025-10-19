use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{Expr, ItemFn, LitStr, Result, meta, parse::Parser, parse_macro_input};

struct InstrumentOptions {
    kind: Option<Expr>,
    fields: Vec<proc_macro2::TokenStream>,
}

impl InstrumentOptions {
    pub fn parse(options: TokenStream) -> Result<Self> {
        let mut kind = None;
        let mut fields = vec![];

        let parser = meta::parser(|meta| {
            macro_rules! error {
                ($($t:tt)+) => {
                    return Err(meta.error(format_args!($($t)+)))
                };
            }

            let Some(ident) = meta.path.get_ident() else {
                error!("unsupported macro `instrument` option.");
            };

            match ident.to_string().as_str() {
                "labels" => {
                    let mut kv = vec![];

                    meta.parse_nested_meta(|meta| {
                        let Some(ident) = meta.path.get_ident() else {
                            error!("expect label `name`.");
                        };

                        let value: LitStr = meta.value()?.parse()?;

                        kv.push(quote! { (stringify!(#ident), #value) });

                        Ok(())
                    })?;

                    fields.push(
                        quote! { #ident: Some(&[("rust_module_path",module_path!()), #(#kv),*]) },
                    );

                    Ok(())
                }
                "kind" => {
                    let value: proc_macro2::TokenStream = match meta.value() {
                        Ok(value) => {
                            if kind.is_some() {
                                error!("repeated 'instrument' option 'kind'");
                            }
                            let expr: Expr = value.parse()?;
                            kind = Some(expr.clone());
                            quote! { #ident: Some(#expr) }
                        }
                        Err(_) => {
                            quote! { #ident: None }
                        }
                    };

                    fields.push(value);

                    return Ok(());
                }
                _ => {
                    let value: proc_macro2::TokenStream = match meta.value() {
                        Ok(value) => {
                            let expr: Expr = value.parse()?;
                            quote! { #ident: Some(#expr) }
                        }
                        Err(_) => {
                            quote! { #ident: None }
                        }
                    };

                    fields.push(value);

                    return Ok(());
                }
            }
        });

        parser.parse(options)?;

        Ok(Self { kind, fields })
    }
}

/// Create measuring instruments for methods via attribute
#[proc_macro_attribute]
pub fn instrument(options: TokenStream, item: TokenStream) -> TokenStream {
    let InstrumentOptions { kind, fields } = match InstrumentOptions::parse(options) {
        Ok(options) => options,
        Err(err) => return err.into_compile_error().into(),
    };

    let ItemFn {
        attrs,
        vis,
        sig,
        block,
    } = parse_macro_input!(item as ItemFn);

    let block = if sig.asyncness.is_some() {
        quote! {
            async #block.await
        }
    } else {
        quote!(#block)
    };

    let make_counter = || {
        quote! {
            #(#attrs)*
            #vis #sig {
                static COUNTER: std::sync::LazyLock<Option<metricrs::Counter>> = std::sync::LazyLock::new(|| {
                    metricrs::global::get_global_registry().map(|registry| {
                        use metricrs::*;
                        use DeriveKind::*;
                        registry.counter(DeriveOption {
                            #(#fields,)*
                            ..Default::default()
                        }.into())
                    })
                });

                if let Some(counter) = COUNTER.as_ref() {
                    let r = #block;
                    counter.increment(1);
                    r
                } else {
                    #block
                }
            }
        }
    };

    let make_timer = || {
        quote! {
            #(#attrs)*
            #vis #sig {

                static TIMER: std::sync::LazyLock<Option<metricrs::Histogram>> = std::sync::LazyLock::new(|| {
                    metricrs::global::get_global_registry().map(|registry| {
                        use metricrs::*;
                        use DeriveKind::*;
                        registry.histogam(DeriveOption {
                              #(#fields,)*
                            ..Default::default()
                        }.into())
                    })
                });

                if let Some(timer) = TIMER.as_ref() {
                    let now = std::time::Instant::now();
                    let r = #block;
                    timer.record(now.elapsed().as_secs_f64());
                    r
                } else {
                    #block
                }
            }
        }
    };

    if let Some(kind) = kind {
        match kind.to_token_stream().to_string().as_str() {
            "Timer" => return make_timer().into(),
            _ => return make_counter().into(),
        }
    }

    return make_counter().into();
}
