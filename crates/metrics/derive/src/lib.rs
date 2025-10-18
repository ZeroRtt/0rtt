use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{Error, Ident, ItemFn, LitStr, Token, braced, parse::Parse, parse_macro_input};

struct InstrumentArgs {
    pub kind: (Ident, Ident),
    pub name: (Ident, LitStr),
    pub labels: Vec<(Ident, LitStr)>,
}

impl Parse for InstrumentArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let span = input.span();

        let mut kind = None;
        let mut name = None;
        let mut labels = None;

        loop {
            let ident: Ident = input.parse()?;

            match ident.to_token_stream().to_string().as_str() {
                "kind" => {
                    if kind.is_some() {
                        return Err(Error::new(ident.span(), "Provide argument `kind` twice."));
                    }

                    let _: Token![=] = input.parse()?;

                    let value: Ident = input.parse().map_err(|err| {
                        Error::new(
                            err.span(),
                            "Provide instrument type: `Counter`,`Timer`, ...",
                        )
                    })?;

                    kind = Some((ident, value));
                }
                "name" => {
                    if name.is_some() {
                        return Err(Error::new(ident.span(), "Provide argument `name` twice."));
                    }

                    let _: Token![=] = input.parse()?;

                    let value: LitStr = input
                        .parse()
                        .map_err(|err| Error::new(err.span(), "Provide instrument name."))?;

                    name = Some((ident, value));
                }
                "labels" => {
                    if labels.is_some() {
                        return Err(Error::new(ident.span(), "Provide argument `labels` twice."));
                    }

                    let content;

                    braced!(content in input);

                    let mut kv = vec![];

                    loop {
                        let key: Ident = content.parse()?;

                        let _: Token![:] = content.parse()?;

                        let value: LitStr = content.parse()?;

                        kv.push((key, value));

                        let Some(_): Option<Token![,]> = content.parse()? else {
                            break;
                        };
                    }

                    labels = Some(kv);
                }
                _ => {
                    return Err(Error::new(
                        ident.span(),
                        "Unknown argument, expect: `kind`, `name` or `labels`",
                    ));
                }
            }

            let Some(_): Option<Token![,]> = input.parse()? else {
                break;
            };
        }

        Ok(Self {
            kind: kind.ok_or_else(|| Error::new(span, "Required parameter `kind`"))?,
            name: name.ok_or_else(|| Error::new(span, "Required parameter `name`"))?,
            labels: labels.unwrap_or_default(),
        })
    }
}

/// Create measuring instruments for methods via attribute
#[proc_macro_attribute]
pub fn instrument(attrs: TokenStream, item: TokenStream) -> TokenStream {
    let InstrumentArgs { kind, name, labels } = parse_macro_input!(attrs as InstrumentArgs);

    let (kind_ident, kind_value) = kind;
    let (name_ident, name_value) = name;

    let labels = labels
        .into_iter()
        .map(|(key, value)| {
            quote! {
                (stringify!(#key),#value)
            }
        })
        .collect::<Vec<_>>();

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

    if kind_value == "Counter" {
        quote! {
            #(#attrs)*
            #vis #sig {

                static COUNTER: std::sync::LazyLock<Option<metricrs::Counter>> = std::sync::LazyLock::new(|| {
                    metricrs::global::get_global_registry().map(|registry| {
                        registry.counter(metricrs::Token {
                            #kind_ident: metricrs::Kind::#kind_value,
                            #name_ident: #name_value,
                            labels: &[("rust_module_path",module_path!()),#(#labels),*],
                        })
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
        .into()
    } else if kind_value == "Timer" {
        quote! {
            #(#attrs)*
            #vis #sig {

                static TIMER: std::sync::LazyLock<Option<metricrs::Histogram>> = std::sync::LazyLock::new(|| {
                    metricrs::global::get_global_registry().map(|registry| {
                        registry.histogam(metricrs::Token {
                            #kind_ident: metricrs::Kind::#kind_value,
                            #name_ident: #name_value,
                            labels: &[("rust_module_path",module_path!()),#(#labels),*]
                        })
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
        .into()
    } else {
        Error::new(
            kind_value.span(),
            "invalid instrument type, see `metricrs::Kind` for more information.",
        )
        .into_compile_error()
        .into()
    }
}
