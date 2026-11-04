use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{
    Error, FnArg, GenericArgument, ImplItem, ImplItemFn, ItemImpl, Meta, PathArguments, ReturnType,
    Type, parse::Parser, spanned::Spanned,
};

pub fn expand(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let mut aggregate: Option<Type> = None;
    syn::meta::parser(|meta| {
        if !meta.path.is_ident("aggregate") {
            return Err(meta.error("expected `aggregate = AggregateType`"));
        }
        crate::support::set_once(
            &mut aggregate,
            meta.value()?.parse()?,
            &meta.path,
            "aggregate",
        )
    })
    .parse2(args)?;
    let mut implementation: ItemImpl = syn::parse2(input)?;
    let aggregate = aggregate.ok_or_else(|| {
        Error::new_spanned(
            &implementation.self_ty,
            "event_codec requires `aggregate = AggregateType`",
        )
    })?;
    if implementation.trait_.is_some() || implementation.unsafety.is_some() {
        return Err(Error::new_spanned(
            &implementation.self_ty,
            "event_codec requires an inherent impl",
        ));
    }
    if !implementation.generics.params.is_empty() || implementation.generics.where_clause.is_some()
    {
        return Err(Error::new_spanned(
            &implementation.generics,
            "event_codec does not support generic impls",
        ));
    }
    let mut registrations = Vec::new();
    for item in &mut implementation.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };
        if method.sig.ident == "builder" {
            return Err(Error::new_spanned(
                &method.sig.ident,
                "event_codec reserves the method name `builder`",
            ));
        }
        let markers: Vec<_> = method
            .attrs
            .iter()
            .filter(|attribute| attribute.path().is_ident("upcast"))
            .collect();
        let Some((marker, remaining)) = markers.split_first() else {
            continue;
        };
        if !remaining.is_empty() || !matches!(marker.meta, Meta::Path(_)) {
            return Err(Error::new_spanned(
                marker,
                "use exactly one `#[upcast]` marker without arguments",
            ));
        }
        let registration = register(method)?;
        method
            .attrs
            .retain(|attribute| !attribute.path().is_ident("upcast"));
        registrations.push(registration);
    }
    let self_ty = &implementation.self_ty;
    // The crate-local bridge works with the facade, runtime, and renamed dependencies.
    let attributes = &implementation.attrs;
    Ok(quote! {
        #implementation
        #(#attributes)*
        impl #self_ty {
            pub fn builder() -> crate::__rostfrei_macro_support::EventCodecBuilder<#aggregate> {
                let builder = crate::__rostfrei_macro_support::EventCodecBuilder::<#aggregate>::new();
                #(#registrations)*
                builder
            }
        }
    })
}

fn register(method: &ImplItemFn) -> syn::Result<TokenStream> {
    let signature = &method.sig;
    if signature.asyncness.is_some() {
        return Err(Error::new_spanned(
            signature.asyncness,
            "upcast conversions must be synchronous",
        ));
    }
    if signature.unsafety.is_some() || signature.abi.is_some() || signature.variadic.is_some() {
        return Err(Error::new_spanned(
            signature,
            "upcast conversions must be safe Rust functions",
        ));
    }
    if !signature.generics.params.is_empty() || signature.generics.where_clause.is_some() {
        return Err(Error::new_spanned(
            &signature.generics,
            "upcast conversions cannot be generic",
        ));
    }
    if signature.receiver().is_some() {
        return Err(Error::new_spanned(
            signature.receiver(),
            "upcast conversions cannot take a receiver",
        ));
    }
    let mut inputs = signature.inputs.iter();
    let Some(FnArg::Typed(input)) = inputs.next() else {
        return Err(Error::new_spanned(
            signature,
            "upcast conversions require exactly one owned historical event",
        ));
    };
    if inputs.next().is_some() || !matches!(&*input.ty, Type::Path(_)) {
        return Err(Error::new_spanned(
            &signature.inputs,
            "upcast conversions require exactly one owned historical event",
        ));
    }
    let ReturnType::Type(_, output) = &signature.output else {
        return Err(Error::new_spanned(
            signature,
            "upcast conversions must return a current event or Result<CurrentEvent, Error>",
        ));
    };
    let Type::Path(output_path) = &**output else {
        return Err(Error::new_spanned(
            output,
            "upcast conversions must return a current event or Result<CurrentEvent, Error>",
        ));
    };
    let name = &signature.ident;
    let input = &input.ty;
    let cfg = method
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("cfg"));
    if let Some(attribute) = method
        .attrs
        .iter()
        .find(|attribute| attribute.path().is_ident("cfg_attr"))
    {
        return Err(Error::new_spanned(
            attribute,
            "use `#[cfg(...)]` directly on upcast conversions instead of cfg_attr",
        ));
    }
    if let Some(segment) = output_path.path.segments.last()
        && segment.ident == "Result"
    {
        if let PathArguments::AngleBracketed(arguments) = &segment.arguments {
            let mut arguments = arguments.args.iter();
            if let (
                Some(GenericArgument::Type(current)),
                Some(GenericArgument::Type(error)),
                None,
            ) = (arguments.next(), arguments.next(), arguments.next())
            {
                return Ok(quote_spanned! {output.span()=>
                    #(#cfg)*
                    let builder = builder.try_upcast::<#input, #current, #error>(Self::#name);
                });
            }
        }
        return Err(Error::new_spanned(
            output,
            "fallible upcast conversions require Result<CurrentEvent, Error>",
        ));
    }
    Ok(quote_spanned! {output.span()=>
        #(#cfg)*
        let builder = builder.upcast::<#input, #output>(Self::#name);
    })
}
