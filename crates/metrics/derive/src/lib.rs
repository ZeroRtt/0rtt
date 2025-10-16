use std::fmt::Display;

use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{Ident, ItemFn, LitStr, Token, parse::Parse, parse_macro_input};

struct InstrumentArgs {
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
        let name = input.parse()?;
        let mut tags = vec![];

        while let Some(_) = Option::<Token![,]>::parse(input)? {
            let tag_name: Ident = input.parse()?;
            let _: Token![=] = input.parse()?;
            let value: LitStr = input.parse()?;
            tags.push((tag_name, value));
        }

        Ok(Self { name, labels: tags })
    }
}

/// Register `counter` measuring instrument for this function.
#[proc_macro_attribute]
pub fn counter(attrs: TokenStream, item: TokenStream) -> TokenStream {
    let InstrumentArgs { name, labels } = parse_macro_input!(attrs as InstrumentArgs);

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

    quote! {
        #(#attrs)*
        #vis #sig {
            let r = #block;

            let labels = vec![#(#labels),*];
            metricrs::global::get_global_registry().counter(#name, &labels).increment(1);
            r
        }
    }
    .into()
}
