use syn::spanned::Spanned;
use syn::visit_mut::VisitMut;
use syn::{Data, DataEnum, DeriveInput, Field, Fields, LitStr, Type, TypeReference, Variant};

use super::ir::{NamedField, Outcome, Shape, ValueField};

pub fn validate(input: &DeriveInput) -> syn::Result<&DataEnum> {
    reject_misplaced_attributes(&input.attrs)?;
    validate_generics(input)?;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "DecisionOutcome only supports non-generic enums",
        ));
    };
    validate_enum(data)?;
    Ok(data)
}

pub fn normalize(data: &DataEnum) -> syn::Result<Vec<Outcome>> {
    data.variants.iter().map(normalize_variant).collect()
}

fn validate_generics(input: &DeriveInput) -> syn::Result<()> {
    if input.generics.params.is_empty() && input.generics.where_clause.is_none() {
        return Ok(());
    }
    Err(syn::Error::new_spanned(
        &input.generics,
        "DecisionOutcome only supports non-generic enums",
    ))
}

fn validate_enum(data: &DataEnum) -> syn::Result<()> {
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            data.enum_token,
            "DecisionOutcome enums must declare at least one variant",
        ));
    }
    for variant in &data.variants {
        if let Some((_, discriminant)) = &variant.discriminant {
            return Err(syn::Error::new_spanned(
                discriminant,
                "DecisionOutcome variants cannot have explicit discriminants",
            ));
        }
        for field in &variant.fields {
            validate_field(field)?;
        }
    }
    Ok(())
}

fn validate_field(field: &Field) -> syn::Result<()> {
    reject_misplaced_attributes(&field.attrs)?;
    reject_references(&field.ty)
}

fn reject_misplaced_attributes(attributes: &[syn::Attribute]) -> syn::Result<()> {
    if let Some(attribute) = attributes
        .iter()
        .find(|attribute| attribute.path().is_ident("outcome"))
    {
        return Err(syn::Error::new_spanned(
            attribute,
            "outcome attributes are only supported on enum variants",
        ));
    }
    Ok(())
}

fn reject_references(ty: &Type) -> syn::Result<()> {
    let mut syntax = ty.clone();
    let mut visitor = ReferenceVisitor { error: None };
    visitor.visit_type_mut(&mut syntax);
    visitor.error.map_or(Ok(()), Err)
}

struct ReferenceVisitor {
    error: Option<syn::Error>,
}

impl VisitMut for ReferenceVisitor {
    fn visit_type_reference_mut(&mut self, reference: &mut TypeReference) {
        if self.error.is_none() {
            self.error = Some(syn::Error::new_spanned(
                reference,
                "references are not supported in DecisionOutcome payload fields",
            ));
        }
    }
}

fn normalize_variant(variant: &Variant) -> syn::Result<Outcome> {
    let attribute = super::attributes::parse(variant)?;
    let shape = match &variant.fields {
        Fields::Unit => Shape::Unit,
        Fields::Unnamed(fields) => Shape::Tuple {
            fields: fields.unnamed.iter().map(normalize_value_field).collect(),
        },
        Fields::Named(fields) => Shape::Struct {
            fields: fields
                .named
                .iter()
                .map(normalize_named_field)
                .collect::<syn::Result<_>>()?,
        },
    };
    Ok(Outcome {
        local_id: attribute.id,
        label: attribute.label,
        shape,
        cfg_attributes: super::cfg_attributes::collect(&variant.attrs),
    })
}

fn normalize_named_field(field: &Field) -> syn::Result<NamedField> {
    let Some(identifier) = &field.ident else {
        return Err(syn::Error::new(
            field.ty.span(),
            "named DecisionOutcome field is missing an identifier",
        ));
    };
    let authored_name = identifier.to_string();
    let name = authored_name
        .strip_prefix("r#")
        .unwrap_or(&authored_name)
        .to_owned();
    Ok(NamedField {
        name: LitStr::new(&name, identifier.span()),
        value: normalize_value_field(field),
    })
}

fn normalize_value_field(field: &Field) -> ValueField {
    ValueField {
        ty: field.ty.clone(),
        cfg_attributes: super::cfg_attributes::collect(&field.attrs),
    }
}
