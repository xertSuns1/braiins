// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::DeriveInput;

/// Generates implementation of `node::Info` and `node::Stats` traits for a type marked by this
/// derive.
#[proc_macro_derive(MiningNode, attributes(member_mining_stats))]
pub fn derive_mining_node(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).unwrap();
    impl_derive_mining_node(&ast, "MiningNode", "member_mining_stats").into()
}

fn impl_derive_mining_node(
    ast: &DeriveInput,
    derive_name: &str,
    stats_name: &str,
) -> proc_macro2::TokenStream {
    let name = &ast.ident;
    let generics = &ast.generics;

    let fields = get_fields(&ast, derive_name);
    let mining_stats = find_member(&fields, stats_name);

    quote! {
        impl#generics node::Info for #name#generics {}

        impl#generics node::Stats for #name#generics {
            #[inline]
            fn mining_stats(&self) -> &stats::Mining {
                &self.#mining_stats
            }
        }
    }
}

/// Generates implementation of `node::Stats` and `node::ClientStats` traits
/// for a type marked by this derive.
#[proc_macro_derive(ClientNode, attributes(member_client_stats))]
pub fn derive_client_node(input: TokenStream) -> TokenStream {
    let derive_name = "ClientNode";
    let ast: DeriveInput = syn::parse(input).unwrap();
    let stream = impl_derive_mining_node(&ast, derive_name, "member_client_stats");
    impl_derive_client_node(&ast, stream, derive_name).into()
}

fn impl_derive_client_node(
    ast: &DeriveInput,
    mut stream: proc_macro2::TokenStream,
    derive_name: &str,
) -> proc_macro2::TokenStream {
    let name = &ast.ident;
    let generics = &ast.generics;

    let fields = get_fields(&ast, derive_name);
    let client_stats = find_member(&fields, "member_client_stats");

    stream.extend(quote! {
        impl#generics node::ClientStats for #name#generics {
            #[inline]
            fn client_stats(&self) -> &stats::Client {
                &self.#client_stats
            }
        }
    });
    stream
}

/// Generates implementation of `node::Stats`, `node::WorkSolver` and `node::WorkSolverStats` traits
/// for a type marked by this derive.
#[proc_macro_derive(WorkSolverNode, attributes(member_work_solver_stats))]
pub fn derive_work_solver_node(input: TokenStream) -> TokenStream {
    let derive_name = "WorkSolverNode";
    let ast: DeriveInput = syn::parse(input).unwrap();
    let stream = impl_derive_mining_node(&ast, derive_name, "member_work_solver_stats");
    impl_derive_work_solver_node(&ast, stream, derive_name).into()
}

fn impl_derive_work_solver_node(
    ast: &DeriveInput,
    mut stream: proc_macro2::TokenStream,
    derive_name: &str,
) -> proc_macro2::TokenStream {
    let name = &ast.ident;
    let generics = &ast.generics;

    let fields = get_fields(&ast, derive_name);
    let work_solver_stats = find_member(&fields, "member_work_solver_stats");

    stream.extend(quote! {
        impl#generics node::WorkSolver for #name#generics {}

        impl#generics node::WorkSolverStats for #name#generics {
            #[inline]
            fn work_solver_stats(&self) -> &stats::WorkSolver {
                &self.#work_solver_stats
            }
        }
    });
    stream
}

/// Generates implementation of `stats::Mining` trait for a type marked by this derive.
#[proc_macro_derive(
    MiningStats,
    attributes(
        member_start_time,
        member_last_share,
        member_best_share,
        member_valid_network_diff,
        member_valid_job_diff,
        member_valid_backend_diff,
        member_error_backend_diff
    )
)]
pub fn derive_mining_stats(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).unwrap();
    impl_derive_mining_stats(&ast, "MiningStats").into()
}

fn impl_derive_mining_stats(ast: &DeriveInput, derive_name: &str) -> proc_macro2::TokenStream {
    let name = &ast.ident;
    let generics = &ast.generics;

    let fields = get_fields(&ast, derive_name);
    let start_time = find_member(&fields, "member_start_time");
    let last_share = find_member(&fields, "member_last_share");
    let best_share = find_member(&fields, "member_best_share");
    let valid_network_diff = find_member(&fields, "member_valid_network_diff");
    let valid_job_diff = find_member(&fields, "member_valid_job_diff");
    let valid_backend_diff = find_member(&fields, "member_valid_backend_diff");
    let error_backend_diff = find_member(&fields, "member_error_backend_diff");

    quote! {
        impl#generics stats::Mining for #name#generics {
            #[inline]
            fn start_time(&self) -> &std::time::Instant {
                &self.#start_time
            }

            #[inline]
            fn last_share(&self) -> &stats::LastShare {
                &self.#last_share
            }

            #[inline]
            fn best_share(&self) -> &stats::BestShare {
                &self.#best_share
            }

            #[inline]
            fn valid_network_diff(&self) -> &stats::Meter {
                &self.#valid_network_diff
            }

            #[inline]
            fn valid_job_diff(&self) -> &stats::Meter {
                &self.#valid_job_diff
            }

            #[inline]
            fn valid_backend_diff(&self) -> &stats::Meter {
                &self.#valid_backend_diff
            }

            #[inline]
            fn error_backend_diff(&self) -> &stats::Meter {
                &self.#error_backend_diff
            }
        }
    }
}

/// Generates implementation of `stats::Client` trait for a type marked by this derive.
#[proc_macro_derive(
    ClientStats,
    attributes(
        member_start_time,
        member_valid_jobs,
        member_invalid_jobs,
        member_generated_work,
        member_last_share,
        member_best_share,
        member_accepted,
        member_rejected,
        member_stale,
        member_valid_network_diff,
        member_valid_job_diff,
        member_valid_backend_diff,
        member_error_backend_diff
    )
)]
pub fn derive_client_stats(input: TokenStream) -> TokenStream {
    let derive_name = "ClientStats";
    let ast: DeriveInput = syn::parse(input).unwrap();
    let stream = impl_derive_mining_stats(&ast, derive_name);
    impl_derive_client_stats(&ast, stream, derive_name).into()
}

fn impl_derive_client_stats(
    ast: &DeriveInput,
    mut stream: proc_macro2::TokenStream,
    derive_name: &str,
) -> proc_macro2::TokenStream {
    let name = &ast.ident;
    let generics = &ast.generics;

    let fields = get_fields(&ast, derive_name);
    let valid_jobs = find_member(&fields, "member_valid_jobs");
    let invalid_jobs = find_member(&fields, "member_invalid_jobs");
    let generated_work = find_member(&fields, "member_generated_work");
    let accepted = find_member(&fields, "member_accepted");
    let rejected = find_member(&fields, "member_rejected");
    let stale = find_member(&fields, "member_stale");

    stream.extend(quote! {
        impl#generics stats::Client for #name#generics {
            #[inline]
            fn valid_jobs(&self) -> &stats::CounterUsize {
                &self.#valid_jobs
            }

            #[inline]
            fn invalid_jobs(&self) -> &stats::CounterUsize {
                &self.#invalid_jobs
            }

            #[inline]
            fn generated_work(&self) -> &stats::CounterU64 {
                &self.#generated_work
            }

            #[inline]
            fn accepted(&self) -> &stats::Meter {
                &self.#accepted
            }

            #[inline]
            fn rejected(&self) -> &stats::Meter {
                &self.#rejected
            }

            #[inline]
            fn stale(&self) -> &stats::Meter {
                &self.#stale
            }
        }
    });
    stream
}

/// Generates implementation of `stats::WorkSolver` trait for a type marked by this derive.
#[proc_macro_derive(
    WorkSolverStats,
    attributes(
        member_start_time,
        member_last_work_time,
        member_generated_work,
        member_last_share,
        member_best_share,
        member_valid_network_diff,
        member_valid_job_diff,
        member_valid_backend_diff,
        member_error_backend_diff
    )
)]
pub fn derive_work_solver_stats(input: TokenStream) -> TokenStream {
    let derive_name = "WorkSolverStats";
    let ast: DeriveInput = syn::parse(input).unwrap();
    let stream = impl_derive_mining_stats(&ast, derive_name);
    impl_derive_work_solver_stats(&ast, stream, derive_name).into()
}

fn impl_derive_work_solver_stats(
    ast: &DeriveInput,
    mut stream: proc_macro2::TokenStream,
    derive_name: &str,
) -> proc_macro2::TokenStream {
    let name = &ast.ident;
    let generics = &ast.generics;

    let fields = get_fields(&ast, derive_name);
    let last_work_time = find_member(&fields, "member_last_work_time");
    let generated_work = find_member(&fields, "member_generated_work");

    stream.extend(quote! {
        impl#generics stats::WorkSolver for #name#generics {
            #[inline]
            fn last_work_time(&self) -> &stats::Timestamp {
                &self.#last_work_time
            }

            #[inline]
            fn generated_work(&self) -> &stats::CounterU64 {
                &self.#generated_work
            }
        }
    });
    stream
}

fn get_fields<'a>(ast: &'a DeriveInput, derive_name: &str) -> &'a syn::Fields {
    match ast.data {
        syn::Data::Struct(ref data) => &data.fields,
        _ => panic!(
            "#[derive({})] can only be used with braced structs",
            derive_name
        ),
    }
}

fn find_member<'a>(fields: &'a syn::Fields, member: &str) -> &'a syn::Ident {
    for field in fields {
        for attr in &field.attrs {
            if attr.path.is_ident(member) {
                return field.ident.as_ref().expect("missing member");
            }
        }
    }
    panic!("missing `{}` attribute", member);
}
