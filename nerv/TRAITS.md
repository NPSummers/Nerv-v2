# Static traits and implementations

Traits are compile-time contracts. Generic specialization resolves constrained operations to concrete method symbols; checked IR has no trait objects, vtables, or dynamic dispatch.

```nerv
trait Read
    fn read(self) -> i64

struct Value implements Read
    number: i64
    fn read(self) -> i64 => self.number

fn fetch<T: Read>(value: T) -> i64 => value:read()

fn main() -> i64 => fetch(Value { number: 42 })
```

Alternatively, declare the methods separately:

```nerv
trait Read
    fn read(self) -> i64

struct Value
    number: i64

impl Value for Read
    fn read(self) -> i64 => self.number

fn main() -> i64 => Value { number: 42 }:read()
```

The current syntax puts the implementing type first in `impl Value for Read`. An `impl Value` block without `for` adds inherent methods without a trait claim. Generic nominal types can declare `implements` inline; their instantiated methods are checked with the owner's concrete type bindings.

## Resolution and validation

Trait definitions are registered before types and implementation claims. Explicit claims are collected before function signatures require constrained types. Inline claims and method signatures are attached to concrete owner symbols. This permits forward declarations while preserving duplicate-implementation checks.

A bound resolves the trait in the declaration's source module. A type must have the corresponding claim; merely having a similarly named method is insufficient. Same-named traits in different modules retain distinct identities. Numeric and other non-nominal values cannot satisfy these nominal trait constraints.

Claim validation substitutes the implementing type for the trait's receiver type, then compares concrete parameter/result signatures. Methods can return the implementing type through the trait's own receiver name. Validation runs after signatures are collected and again after specialization, covering claims created by generic type instantiation. Generic function and nominal-type bounds share this mechanism.

## Compatibility and limits

The stage-0 compiler checks claims for generic bounds but accepts some missing or incompatible claimed methods when they are unused. This implementation is deliberately stricter: claimed contracts must match their methods even without a call. Two separate regression cases record that difference rather than claiming complete stage-0 parity.

Trait methods are signature-validated when a concrete implementation claims them. This does not add exhaustive validation of every unused trait signature or every unused generic body. Trait objects, generic trait declarations, multiple simultaneous bounds, associated types, default method bodies, and applied-generic `impl Box<T>` syntax are not provided by this stage. The existing parser still determines which declaration forms are accepted.

Tests cover inline/explicit/inherent implementations, declaration order, nominal constraints, specialized generic owners, method signatures, duplicate traits/claims, and namespaced trait identity. Project manifests, C interoperability, and LLVM lowering are separate work.
