use proc_macro2::TokenStream;
use quote::quote;

use crate::StructData;

pub(crate) fn variables(data: &StructData) -> TokenStream {
    let recurse = data.fields.iter().map(|(name, ty, _)| {
        quote! {
            vars_vec.extend_from_slice(<#ty as CircuitVariable>::variables(&self.#name).as_slice());

        }
    });
    quote! {
        let mut vars_vec = vec![];

        #(#recurse)*

        vars_vec
    }
}

pub(crate) fn from_variables(data: &StructData) -> TokenStream {
    let value_recurse = data.fields.iter().map(|(name, ty, _)| {
        quote! {
            let size = <#ty as CircuitVariable>::nb_elements();
            let #name = <#ty as CircuitVariable>::from_variables(&variables[index..index+size]);
            index += size;
        }
    });

    let instant_recurse = data.fields.iter().map(|(name, _, _)| {
        quote! {
            #name,
        }
    });
    quote! {
        let mut index = 0;
        #(#value_recurse)*

        Self {
            #(#instant_recurse)*
        }
    }
}
