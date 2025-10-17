use std::fmt::Display;

use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{Error, Ident, ItemFn, LitStr, Token, parse::Parse, parse_macro_input};

struct InstrumentArgs {
    pub ident: Ident,
    pub name: LitStr,
    pub labels: Vec<(Ident, LitStr)>,
}

impl Display for InstrumentArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "instrument({}", self.name.to_token_stream())?;
        for (name, value) in &self.labels {
            write!(f, ", {}={}", name, value.to_token_stream())?;
        }
        write!(f, ")")
    }
}

impl Parse for InstrumentArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let ident = input.parse().map_err(|err| {
            Error::new(
                err.span(),
                "expect instrument `Type` argument: counter, timer, ..",
            )
        })?;

        let _: Token![,] = input
            .parse()
            .map_err(|err| Error::new(err.span(), "expect instrument `Name` argument"))?;

        let name = input
            .parse()
            .map_err(|err| Error::new(err.span(), "expect instrument `Name` argument"))?;

        let mut tags = vec![];

        while let Some(_) = Option::<Token![,]>::parse(input)? {
            let tag_name: Ident = input.parse()?;
            let _: Token![=] = input.parse()?;
            let value: LitStr = input.parse()?;
            tags.push((tag_name, value));
        }

        Ok(Self {
            ident,
            name,
            labels: tags,
        })
    }
}

/// Create measuring instruments for methods via attribute
#[proc_macro_attribute]
pub fn instrument(attrs: TokenStream, item: TokenStream) -> TokenStream {
    let InstrumentArgs {
        ident,
        name,
        labels,
    } = parse_macro_input!(attrs as InstrumentArgs);

    let labels = labels
        .into_iter()
        .map(|(key, value)| {
            let key = key.to_string();
            quote! {
                (#key,#value)
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

    if ident == "counter" {
        quote! {
            #(#attrs)*
            #vis #sig {

                static TOKEN: std::sync::LazyLock<metricrs::Token<'static>> = std::sync::LazyLock::new(|| {
                    metricrs::Token::new(#name, &[("rust_module_path",module_path!()),#(#labels),*])
                });

                if let Some(registry) = metricrs::global::get_global_registry() {
                    let r = #block;
                    registry.counter(*TOKEN).increment(1);
                    r
                } else {
                    #block
                }
            }
        }
        .into()
    } else if ident == "timer" {
        quote! {
            #(#attrs)*
            #vis #sig {

                static TOKEN: std::sync::LazyLock<metricrs::Token<'static>> = std::sync::LazyLock::new(|| {
                    metricrs::Token::new(#name, &[("rust_module_path",module_path!()),#(#labels),*])
                });

                if let Some(registry) = metricrs::global::get_global_registry() {
                    let now = std::time::Instant::now();
                    let r = #block;
                    registry.histogam(*TOKEN).record(now.elapsed().as_secs_f64());
                    r
                } else {
                    #block
                }
            }
        }
        .into()
    } else {
        Error::new(
            ident.span(),
            "invalid instrument type, expect: counter, timer, ..",
        )
        .into_compile_error()
        .into()
    }
}
