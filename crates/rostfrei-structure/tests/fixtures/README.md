# Structure checker fixtures

Each fixture is a Rust source tree whose configured domain root is `src/domain`.
The sources only need to parse with `syn`; they intentionally do not form
standalone Cargo packages.

| Fixture family | Expected result |
|---|---|
| `valid_domain`, `content_valid_role_impls`, `test_mirror_valid` | pass |
| `hierarchy_*` | `RF001` |
| `logic_in_mod` | `RF002` |
| `macro_in_wrong_file` | `RF003` |
| `multiple_primary_declarations` | `RF004` |
| `test_outside_tests` | `RF005` |
| `undeclared_file`, `missing_module_file` | `RF006` |
| `content_*` except `content_valid_role_impls` | `RF007` |
| `test_mirror_*` except `test_mirror_valid` | `RF008` |
| `value_object_conventions` | focused `RF001`, `RF003`, `RF004`, and `RF007` |

`valid_domain` demonstrates the agreed hierarchy: bounded context, aggregate,
nested entity and capability objects, invariant and lifecycle objects, and a
sibling `tests` tree that mirrors the production domain. It also demonstrates
context-, aggregate-, and entity-owned Value Object modules plus a behaviorful
Value Object with action, decision, and invariant capabilities.

The integration tests contain the exact expected diagnostic path for every
negative fixture. RF009 and RF010 use temporary Cargo workspaces because those
codes validate package targets and compiled execution rather than source trees.
