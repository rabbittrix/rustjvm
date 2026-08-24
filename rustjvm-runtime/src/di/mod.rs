//! The Rust-managed dependency-injection container.
//!
//! Beans are extracted from Java source by `rustjvm-compiler`
//! (`@Service`, `@Component`, `@Configuration` + `@Bean`, `@RestController`),
//! wired here by type through `@Autowired` fields, and constructed
//! dependency-first after a topological sort. Cycles, unsatisfiable
//! dependencies, and singleton→request scope violations fail fast at boot;
//! during hot-reload a broken graph keeps the previous registry live.
//!
//! Scopes: singletons are built at boot and shared; prototypes are built
//! fresh per injection; request-scoped beans live in the [`RequestContext`]
//! and are reclaimed with the request arena.

mod registry;

pub use registry::{
    assemble_registry, Bean, BeanRegistry, DIError, NativeDef, NativeFn, RequestContext,
};
