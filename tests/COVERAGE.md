# Test coverage

This checklist maps each section and table row of
[`docs/language.md`](../docs/language.md) to the test programs that exercise
it. Names without a directory are `tests/run/NAME.12am` (with `NAME.out`);
`fail/NAME` is `tests/fail/NAME.12am` (with `NAME.err`); `panic/NAME` is
`tests/panic/NAME.12am` (with `NAME.err`); `plan/NAME` is
`tests/plan/NAME.12am` (with `NAME.plan`).

Every expected output was worked out by hand from the reference. Where the
reference is ambiguous or silent, the reading a test relies on is listed under
[Assumptions](#assumptions), and the test that isolates it is named there.

Counts: 240 programs in `tests/run` (plus 4 helpers in `tests/run/lib`),
333 in `tests/fail`, 75 in `tests/panic`, 10 in `tests/plan`.

## A first program, The model

| Construct | Files |
| --- | --- |
| The first program (two systems, a filtered query, `Time.dt`, three ticks) | `system_first_program` |
| Everything is a value: copied on assignment | `value_copy_lists`, `value_copy_strings`, `value_copy_structs`, `fn_params_are_copies` |
| An `Entity` handle can outlive its entity; `alive?` | `entity_handles`, `entity_dead_handle` |

## Lexical structure

| Construct | Files |
| --- | --- |
| Newline ends a statement; no end inside `( )` and `[ ]` | `lex_continuation` |
| Continuation after a binary operator, a comma, `=`, a compound assignment operator | `lex_continuation` |
| Continuation when the next line begins with `.` | `lex_continuation` |
| Comments: whole line, trailing, `#` inside strings | `lex_comments` |
| Identifiers: lowercase or `_` start, digits, `_` | `lex_names` |
| Function and method names ending in `?` and `!` | `lex_names`, `fn_question_bang_names`, `fn_mutual_recursion`, `struct_methods` |
| `a!=b` is `a != b` | `lex_names` |
| Conditional operator written with spaces after a `?` name | `lex_names`, `fn_question_bang_names`, `fn_mutual_recursion` |
| Keywords are reserved, `gpu` included | `fail/syntax_keyword_as_variable`, `fail/syntax_gpu_reserved` |
| Built-in names can't be redefined | `fail/builtin_type_redefined`, `fail/builtin_compact_type_redefined`, `fail/builtin_name_redefined_constant` |
| `Int` literals: `42`, `1_000_000`, `0xff`, `0b1010` | `lex_int_literals`, `tos_int`, `const_expressions` |
| `Float` literals: `3.14`, `1_000.5`, `1e-3`, `2.5e6` | `lex_float_literals`, `tos_float` |
| `Bool` literals | `types_basic`, `ops_logic` |
| String literals, double and single quoted | `lex_string_escapes`, `lex_string_single` |
| List literals, `[]` | `lex_list_literals` |
| Escapes `\n` `\t` `\\` `\"` `\#` (printed) | `lex_string_escapes` |
| Escapes `\r` `\0` `\e` (checked through `length`, `bytesize`, `==`; never printed) | `lex_string_escapes`, `str_escape_lengths` |
| Interpolation `#{expr}` with every basic type, expressions, calls | `lex_interpolation` |
| Nested interpolation, string literals and `}` inside an interpolation | `lex_interpolation_nested` |
| Single-quoted strings: taken as written, only `\'` and `\\` | `lex_string_single` |

## Types

| Construct | Files |
| --- | --- |
| `Int`: 64-bit, `+ - *` and negation wrap on overflow | `types_int_wrap` |
| `Int`: `**` wraps; smallest Int divided by -1 wraps | `types_int_wrap_power`, `types_int_wrap_division` (assumptions 9, 10) |
| `Float`: 64-bit IEEE 754 | `types_basic`, `ops_float_inexact`, `ops_float_special` |
| `Bool`, `String` (UTF-8) | `types_basic`, `str_length_bytesize` |
| `Entity` | `entity_operations`, `entity_handles` |
| `[T]` for every value type `T` | `types_list_elements`, `tos_list`, `list_include` |
| struct, component, event, enum as value types (variable, field, parameter, list element) | `struct_fields_lists`, `component_as_value`, `struct_like_event`, `enum_basics`, `fn_param_types` |
| Tags and resources are not value types | `fail/tag_in_variable`, `fail/tag_as_field_type`, `fail/resource_as_value`, `fail/resource_as_field_type` |
| Types written for fields, parameters and optional return values; locals inferred | `vars_creation`, `fn_declared_return`, `fn_return_inference` |
| Int to Float: arithmetic with a Float | `conv_int_to_float`, `ops_int_float_mixing` |
| Int to Float: assignment to a Float variable (plain, compound, annotated) | `conv_int_to_float`, `vars_creation` |
| Int to Float: assignment to a Float field (struct, component, resource), defaults, constructor arguments | `conv_int_to_float`, `struct_construct` |
| Int to Float: Float argument (function, `Math`, operator method, `clamp`) | `conv_int_to_float`, `math_functions`, `struct_operators`, `num_clamp` |
| Int to Float: returned from a function that returns Float | `conv_int_to_float`, `fn_int_to_float_return` |
| Int to Float: list literal with an Int among Floats; `push`, element write | `conv_int_to_float`, `conv_list_elements`, `tos_list` |
| Int to Float: list literal of only Ints where `[Float]` is expected | `conv_list_literal` (assumption 5) |
| Int to Float: branches of `if`/`unless`/`case` used as a value | `conv_int_to_float`, `if_expression`, `unless_expression`, `case_expression` |
| Int to Float: `==` and `!=` between an Int and a Float | `conv_int_to_float`, `ops_equality` |
| Float to Int only through `to_i`, `floor`, `ceil`, `round` | `num_conversions`, `fail/assign_float_to_int_variable`, `fail/type_argument_float_to_int` |
| No truthiness | `fail/cond_if_not_bool`, `fail/cond_elsif_not_bool`, `fail/cond_while_not_bool`, `fail/cond_until_not_bool`, `fail/cond_unless_not_bool`, `fail/cond_ternary_not_bool`, `fail/cond_modifier_not_bool`, `fail/cond_modifier_unless_not_bool` |

### Compact field types

| Construct | Files |
| --- | --- |
| `Float32` field: stored rounded to 32 bits, reads as Float | `compact_float32`, `compact_holders`, `layout` |
| `Int8`, `Int16`, `Int32`, `UInt8`, `UInt16`, `UInt32` fields: boundary values stored and read back as Int | `compact_int_fields` |
| Compact fields in a struct, component, resource and event; defaults | `compact_int_fields`, `compact_holders` |
| Arithmetic on values read happens at 64 bits | `compact_int_fields`, `compact_holders`, `compact_lists` |
| `pos.x += vel.dx * Time.dt` on `Float32` fields | `compact_holders` |
| Lists with compact elements (`[UInt8]`, `[Int16]`, `[UInt16]`, `[Int32]`, `[Int8]`, `[UInt32]`, `[Float32]`): fields, annotated variables, parameters; index, write, `push`, `pop`, `+`, iteration, `to_s` | `compact_lists`, `compact_float_lists` |
| `[Float32]` and `[Float]` are different types | `compact_float_lists`, `fail/list_float32_to_float`, `fail/list_float_to_float32`, `fail/list_float32_argument`, `fail/list_uint8_to_int` |
| Compact types not allowed for variables, parameters, return values | `fail/compact_variable`, `fail/compact_float32_variable`, `fail/compact_parameter`, `fail/compact_return` |
| A constant that does not fit is a compile error | `fail/compact_default_too_big`, `fail/compact_default_negative_unsigned`, `fail/compact_constructor_too_big`, `fail/compact_constructor_keyword_too_small`, `fail/compact_assign_too_big`, `fail/compact_assign_int16_too_big`, `fail/compact_uint16_too_big`, `fail/compact_int32_too_big`, `fail/compact_uint32_negative`, `fail/compact_list_literal_too_big`, `fail/compact_list_push_too_big`, `fail/compact_list_element_assign_too_big`, `fail/compact_constant_expression_too_big`, `fail/compact_named_constant_too_big` |
| A computed value that does not fit is a runtime error | `panic/compact_*` (see [Runtime errors](#runtime-errors)) |

## Variables and values

| Construct | Files |
| --- | --- |
| Created by first assignment, keeps its type; annotated forms; `[]` with an annotation | `vars_creation` |
| A variable belongs to its block and nested blocks | `vars_scoping`, `if_statement`, `loop_while` |
| A name is free again once its block has ended | `vars_scope_reuse` |
| Using a variable after the block that created it | `fail/scope_after_if`, `fail/scope_after_while`, `fail/scope_after_do_block`, `fail/scope_block_param_after_block` |
| Block parameters must not reuse a name in scope; sibling blocks may reuse one | `vars_scoping`, `fail/block_param_in_scope`, `fail/block_param_nested_reuse`, `fail/block_param_reuses_function_param`, `fail/block_param_query_reuse`, `fail/block_param_duplicate` |
| Assigning a value of another type to a variable | `fail/assign_different_type`, `fail/assign_float_to_int_variable`, `fail/assign_compound_float_to_int` |
| Assignment copies: lists, nested lists | `value_copy_lists`, `ops_list` |
| Assignment copies: strings | `value_copy_strings` |
| Assignment copies: structs holding lists, strings, structs | `value_copy_structs`, `struct_fields_lists` |
| Place: a local variable, its fields and elements (`path.points[0].x`) | `place_kinds`, `struct_fields_lists`, `list_index_write` |
| Place: a query `each` parameter, whole and by field | `place_kinds`, `query_terms`, `each_field_writes_immediate`, `component_entity` |
| Place: `entity[Component]`, whole and by field | `place_kinds`, `entity_operations` |
| Place: `Resource.field`, and fields and elements of it | `place_kinds`, `resource_fields` |
| Read-only: function and method parameters, `self` | `fail/readonly_function_param`, `fail/readonly_function_param_field`, `fail/readonly_method_param`, `fail/readonly_self_field`, `fail/readonly_self_dot_field` |
| Read-only: constants | `fail/readonly_constant`, `fail/readonly_constant_field`, `fail/readonly_constant_element`, `fail/readonly_constant_in_function`, `fail/const_assigned_twice` |
| Read-only: block parameters of `on`, list iteration, counting loops | `fail/readonly_on_param`, `fail/readonly_on_param_whole`, `fail/readonly_list_iteration_param`, `fail/readonly_list_iteration_param_field`, `fail/readonly_each_with_index_param`, `fail/readonly_counting_param`, `fail/readonly_upto_param` |
| Anything read out of the world is a copy (`h = e[Health]`, `saved = pos` inside each, a resource field) | `place_world_copies`, `refcount` |

## Expressions

| Operator or rule | Files |
| --- | --- |
| Calls, members, indexing, `A::B` bind tightest | `ops_precedence`, `ops_unary_minus` |
| `!` | `ops_logic`, `ops_precedence` |
| `**`: right-associative, tighter than unary minus; `-2 ** 2`, `(-2) ** 2`, `2 ** 3 ** 2`, Float powers, `2.0 ** -1` | `ops_power`, `ops_precedence` |
| Unary `-`; `-1.abs`, `-x.abs`, `-1.upto(1)` | `ops_unary_minus`, `loop_receivers`, `struct_operators` (`-@`) |
| `*`, `/`, `%` on Ints and Floats | `ops_arithmetic`, `ops_int_division`, `ops_modulo` |
| `+`, `-` | `ops_arithmetic` |
| `<`, `<=`, `>`, `>=` on numbers | `ops_compare_numbers`, `ops_int_float_mixing` |
| `<`, `<=`, `>`, `>=` on strings | `ops_compare_strings` (assumption 7) |
| Comparisons do not chain | `fail/comparison_chain` |
| `==`, `!=` on Int, Float, Int vs Float, Bool, String, enum, list, nested list, struct, component value, event value, entity, `Entity.none` | `ops_equality`, `struct_equality_to_s`, `entity_handles` |
| `&&`, `\|\|` short-circuit | `ops_logic` |
| `c ? a : b`, nesting, low precedence | `ops_ternary`, `ops_precedence` |
| Each adjacent pair of precedence levels | `ops_precedence` |
| Assignment is a statement | `fail/syntax_assignment_as_expression` |
| `+=`, `-=`, `*=`, `/=`, `%=` on Int and Float; `+=` on String and list; struct operators | `ops_compound_assign`, `struct_operators`, `str_concat_repeat` |
| Compound assignment through fields, list elements, resource fields, components; the place is located once | `ops_compound_places` |
| Integer division rounds toward negative infinity, all sign combinations | `ops_int_division` |
| `%` takes the sign of the divisor, all sign combinations | `ops_modulo` |
| Float `%` with negative operands | `ops_float_modulo` (assumption 6) |
| Int and Float mixing gives Float | `ops_int_float_mixing` |
| `Int ** Int` is an Int | `ops_power` |
| Inexact Float arithmetic (`0.1 + 0.2`) | `ops_float_inexact` |
| Float division by zero: `Infinity`, `-Infinity`, `NaN` | `ops_float_special`, `num_nan`, `tos_float_special` |
| `+` joins strings and lists; `*` repeats them (`"-" * 20`, `[0] * 64`) | `ops_string`, `ops_list`, `str_concat_repeat` |
| Operands and arguments evaluated left to right | `eval_order`, `eval_keyword_args` |
| Right-hand side evaluated before the place is written; `a += f` reads `a` first | `eval_assignment` |
| `if` as an expression | `if_expression`, `fn_last_expression`, `fn_declared_return`, `fn_recursion` |
| `unless` as an expression | `unless_expression`, `fn_last_expression` |
| `case` as an expression | `case_expression`, `case_enum_exhaustive`, `enum_case_value`, `case_expression_argument` (assumption 20) |
| Branch types must agree; `if` needs an `else` to be a value | `fail/type_if_branches`, `fail/type_case_branches`, `fail/type_ternary_branches`, `fail/value_if_without_else` |
| Operand type errors | `fail/type_operands`, `fail/type_operands_logical`, `fail/type_operands_not`, `fail/type_operands_negate_string`, `fail/type_operands_repeat_count`, `fail/type_operands_struct_no_operator`, `fail/type_operands_operator_method_argument`, `fail/type_operands_struct_on_right`, `fail/type_operands_list_concat`, `fail/type_equality_different_types`, `fail/type_index_not_int`, `fail/compare_bools`, `fail/compare_structs`, `fail/compare_enums` |

## Control flow

| Construct | Files |
| --- | --- |
| `if` / `elsif` / `else` statements | `if_statement` |
| `then` forms: `if … then … else … end`, `elsif … then`, `unless … then` | `if_then` |
| `unless` with and without `else` | `unless_statement`, `if_then` |
| Modifier `if` / `unless` on `puts`, `print`, assignment, compound assignment, calls, element assignment | `modifier_forms` |
| Modifier on `return` and `return value` | `modifier_return`, `fn_recursion`, `fn_mutual_recursion` |
| Modifier on `break`, `next`, `emit` | `modifier_break_next_emit`, `break_*`, `next_*` |
| `while` | `loop_while` |
| `until` | `loop_until` |
| `loop do` | `loop_loop` |
| `case` over an Int | `case_int` |
| `case` over a String | `case_string` |
| `case` over a Bool | `case_bool` |
| `case` over an enum; several candidates per `when`; `else` | `case_enum`, `enum_case` |
| Enum `case` with no `else` covering every variant | `case_enum_exhaustive`, `enum_case`, `enum_case_value` |
| Enum `case` missing variants names them | `fail/case_enum_missing_variants`, `fail/case_enum_missing_one` |
| `case` value or candidate of a wrong type | `fail/case_value_float`, `fail/case_value_struct`, `fail/type_case_when` |
| `times` with and without a parameter, `0.times` | `loop_times` |
| `upto`, `downto`, empty ranges | `loop_upto_downto` |
| Receivers: variable, expression, `xs.length`, negative literal | `loop_receivers`, `ops_unary_minus` |
| List `each` | `loop_list_each` |
| `each_with_index` | `loop_each_with_index` |
| List iteration walks the list as it was when the loop started | `loop_list_snapshot` |
| Updating in place by index | `loop_update_by_index` |
| A `do` block written on one line | `loop_do_one_line` (contradiction 1) |
| `break` in `while`, `until`, `loop` | `break_basic_loops` |
| `break` in `times`, `upto`, `downto` | `break_counting` |
| `break` in list `each`, `each_with_index` | `break_list_each` |
| `break` in a query `each` | `break_query_each`, `each_deferred_break_return` |
| `break` in `on` | `break_on`, `event_break_next` |
| `break` leaves only the nearest loop | `break_nested`, `loop_loop` |
| `next` in `while`, `until`, `loop` | `next_basic_loops` |
| `next` in `times`, `upto`, `downto` | `next_counting` |
| `next` in list `each`, `each_with_index` | `next_list_each` |
| `next` in a query `each` | `next_query_each`, `query_nested`, `each_deferred_despawn` |
| `next` in `on` | `next_on`, `event_break_next`, `event_combat` |
| `next` continues only the nearest loop | `next_nested` |
| `break` / `next` outside a loop | `fail/break_outside_loop`, `fail/next_outside_loop` |
| `do` blocks only on the loop forms | `fail/do_block_not_loop`, `fail/do_block_on_function` |

## Functions

| Construct | Files |
| --- | --- |
| Parameter types required; parameters read-only | `fn_param_types`, `fail/syntax_param_without_type`, `fail/readonly_function_param` |
| Parameters of type Int, Float, String, Bool, enum, struct, list, Entity | `fn_param_types` |
| Last expression is the return value | `fn_last_expression` |
| Early `return`, with and without a value | `fn_early_return`, `modifier_return`, `loop_loop` |
| Return type inferred: Int, Float, String, Bool, list, struct, enum, nothing | `fn_return_inference` |
| Declared return types | `fn_declared_return`, `fn_empty_list_return` (assumption 19) |
| Recursive functions declare their return type (direct, mutual) | `fn_recursion`, `fn_mutual_recursion`, `struct_recursive_list`, `fail/recursion_direct_no_type`, `fail/recursion_mutual_no_type` |
| Zero-parameter functions: declared without parentheses, called as `f` or `f()` | `fn_no_paren_calls`, `loop_until` |
| Calls with arguments use parentheses | `fn_calls_functions`, `fail/syntax_call_without_parentheses` |
| A local hides a function when bare; `name(…)` is always a call | `fn_local_shadows_function`, `fail/unknown_function_local_variable` |
| Use before definition | `fn_use_before_definition`, `const_scopes` |
| Functions may query, spawn, despawn, emit, receive events, use resources | `fn_world_access`, `query_in_functions`, `resource_systems` |
| Arguments are copies | `fn_params_are_copies`, `value_copy_lists` |
| No overloading: one definition per name | `fail/duplicate_function`, `fail/duplicate_system_function` |
| Wrong number or kind of arguments | `fail/args_function_count`, `fail/args_function_too_few`, `fail/args_function_no_params`, `fail/args_method_count`, `fail/args_builtin_count`, `fail/type_argument`, `fail/type_argument_float_to_int` |
| Return type mismatches | `fail/type_return`, `fail/type_return_statement`, `fail/type_return_inconsistent`, `fail/type_return_missing_value` |
| A function that returns nothing used as a value | `fail/void_as_value`, `fail/void_in_expression` |

## Declaring data

| Construct | Files |
| --- | --- |
| Struct: positional and keyword construction, defaults, `Vec2()` | `struct_construct`, `struct_defaults` |
| Struct: defaults of every constant-expression kind | `struct_defaults` |
| Struct: construction errors | `fail/args_constructor_count`, `fail/args_unknown_keyword`, `fail/args_repeated_keyword`, `fail/args_missing_field`, `fail/args_missing_field_positional`, `fail/args_mixed_positional_keyword`, `fail/type_argument_constructor`, `fail/default_not_constant`, `fail/type_field_default` |
| Struct: methods, fields in scope, `self`, `value.name`, `value.name(args)` | `struct_methods`, `lex_names` |
| Struct: methods cannot modify the receiver | `struct_methods`, `fail/readonly_self_field`, `fail/readonly_self_dot_field` |
| Struct: operator methods `+ - * / %` and `-@` | `struct_operators`, `ops_compound_assign` |
| Struct: provided `==` and `to_s`, not redefinable | `struct_equality_to_s`, `tos_struct`, `fail/struct_redefines_to_s`, `fail/struct_redefines_equality` |
| Struct: can't contain itself except through a list | `struct_recursive_list`, `fail/struct_contains_itself`, `fail/struct_contains_itself_mutual` |
| Struct: field and nested field assignment, structs in lists | `struct_fields_lists` |
| Enum: declaration one per line and several per line | `enum_basics`, `enum_case`, `enum_case_value` |
| Enum: `==`, `!=`, `case`, `to_s` | `enum_basics`, `enum_case`, `case_enum` |
| Component: declared, constructed and used like a struct, methods included | `component_as_value`, `component_entity` |
| Component: at least one field | `fail/component_no_fields` |
| Tag: attached, removed, checked, used as a filter | `tag_basics`, `query_filters` |
| Tag: has no value | `fail/tag_constructed`, `fail/tag_in_variable`, `fail/tag_indexed`, `fail/tag_as_field_type`, `fail/query_tag_as_term` |
| Resource: defaults, read and written from top level, functions, systems | `resource_fields`, `resource_systems`, `place_kinds` |
| Resource: every field needs a default; not a value; fields only | `fail/resource_field_no_default`, `fail/resource_as_value`, `fail/resource_as_field_type`, `fail/syntax_resource_method` |
| Event: declared and constructed like a struct; empty body | `struct_like_event`, `event_types_independent` |
| Constants: every type, built from earlier constants | `const_basics` |
| Constants: every constant-expression kind | `const_expressions`, `math_constants` |
| Constants: visible in functions, systems, methods, field defaults | `const_scopes` |
| Constants: can't be changed; rules for defining them | `fail/readonly_constant*`, `fail/const_assigned_twice`, `fail/const_in_function`, `fail/const_not_constant_expression`, `fail/const_uses_later_constant`, `fail/const_uses_variable` |
| Duplicate definitions | `fail/duplicate_type`, `fail/duplicate_type_kinds`, `fail/duplicate_field`, `fail/duplicate_variant`, `fail/duplicate_method`, `fail/duplicate_parameter`, `fail/duplicate_system` |

## Entities

| Construct | Files |
| --- | --- |
| `spawn` with component values; returns the handle | `entity_spawn_forms`, `entity_operations` |
| `spawn` with bare type names: a tag, a component with all defaults | `entity_spawn_forms`, `component_entity` |
| `spawn` alone | `entity_spawn_forms` |
| `spawn(…)` inside a larger expression | `entity_spawn_forms`, `each_deferred_spawn` |
| Listing a type twice; non-components; bare component without defaults | `fail/spawn_type_twice`, `fail/spawn_tag_twice`, `fail/spawn_non_component`, `fail/spawn_bare_no_defaults`, `fail/spawn_bare_partial_defaults` |
| `e[Health]` as a place | `entity_operations`, `place_kinds` |
| `e[Health] = Health(5)` | `entity_operations`, `each_field_writes_immediate` |
| `e.has?(Health)` for components and tags | `entity_operations`, `tag_basics` |
| `e.add(…)`: attach; replace one already present | `entity_operations`, `refcount` |
| `e.add(…)` with bare type names | `entity_add_bare`, `tag_basics`, `fail/add_bare_no_defaults` |
| `e.remove(…)`: detach; absent ones ignored | `entity_operations`, `tag_basics` |
| `e.despawn` | `entity_operations`, `entity_handles` |
| `e.alive?` | `entity_operations`, `entity_handles` |
| `Entity.none` | `entity_dead_handle`, `entity_handles`, `entity_to_s` |
| Handles stored in components, structs and lists; compared | `entity_handles`, `event_combat`, `types_list_elements` |
| A new entity never revives an old handle | `entity_handles`, `entity_dead_handle`, `entity_to_s`, `panic/component_stale_handle` |
| Dead handle: `has?` is false; `add`, `remove`, `despawn` do nothing | `entity_dead_handle`, `each_deferred_order` |
| `e[C]` when dead or missing `C` is a runtime error | `panic/component_*` |
| Entity `to_s`: index and generation, generation after reuse, `Entity(none)` | `entity_to_s` (assumption 1) |
| Non-component arguments | `fail/add_non_component`, `fail/remove_non_component`, `fail/has_non_component`, `fail/index_non_component`, `fail/index_component_wrong_value` |

## Queries

| Construct | Files |
| --- | --- |
| One term, several terms, one block parameter per term | `query_terms` |
| A component parameter is a view: field writes and whole assignment change the world | `query_terms`, `place_kinds`, `each_field_writes_immediate` |
| `Entity` as a term; `each Entity do \|e\|` visits every entity | `query_terms`, `query_filters` |
| `with:` one type and a bracketed list | `query_filters`, `tag_basics` |
| `without:` one type and a bracketed list | `query_filters`, `tag_basics`, `system_first_program` |
| `with:` and `without:` together; components as filters | `query_filters` |
| Compile-time checks | `fail/unknown_component_query_suggest`, `fail/unknown_tag_filter_suggest`, `fail/query_tag_as_term`, `fail/query_non_component`, `fail/query_resource_term`, `fail/query_filter_non_component`, `fail/query_unknown_filter`, `fail/query_repeated_type`, `fail/query_repeated_in_filter`, `fail/query_with_and_without_same`, `fail/query_param_count`, `fail/query_param_count_too_many`, `fail/query_param_count_none`, `fail/query_no_terms` |
| Visit order is deterministic | `query_order_deterministic` |
| Visit order for one component set spawned in sequence | `query_order_spawn` (assumption 2) |
| Queries at the top level, in functions, in systems | `query_in_functions`, `query_terms`, `system_first_program` |
| Nested queries | `query_nested`, `each_deferred_despawn`, `each_deferred_nested` |
| `World.count(…)` with terms and filters | `query_count`, `tag_basics` |
| `World.count` with no arguments | `query_count`, `entity_spawn_forms` |
| `World.count` errors | `fail/count_tag_as_term`, `fail/count_non_component`, `fail/unknown_world_method_suggest` |

## Structural changes inside `each`

| Rule | Files |
| --- | --- |
| Outside a query: immediate (top level, functions, systems, list iteration, counting loops, `while`, `on`) | `each_only_queries_defer`, `system_structural_changes`, `entity_operations` |
| `spawn` inside `each` is queued; the entity is alive with no components yet; the running query does not visit it | `each_deferred_spawn` |
| `despawn` inside `each` is queued; the entity is still alive | `each_deferred_despawn`, `system_structural_changes` |
| `add` and `remove` inside `each` are queued; removed components still present | `each_deferred_add_remove` |
| `has?`, `e[C]`, `World.count` and other queries see the world as it was | `each_deferred_spawn`, `each_deferred_despawn`, `each_deferred_add_remove` |
| Applied in order | `each_deferred_order` |
| Applied when the OUTERMOST `each` finishes | `each_deferred_nested` |
| Includes code in functions called from the block | `each_deferred_nested`, `fn_world_access` |
| Field writes are immediate | `each_field_writes_immediate`, `each_deferred_add_remove` |
| `break`, `next`, `return` out of `each`: the queue is applied when the outermost `each` is left | `each_deferred_break_return`, `refcount` |

## Systems and ticks

| Construct | Files |
| --- | --- |
| A system: own locals, `return` ends it early | `system_order` |
| `World.tick` runs every system once in source order | `system_order`, `system_first_program`, `require_systems` |
| Systems are not called by name; names share a namespace with functions | `fail/system_called`, `fail/system_called_with_parens`, `fail/duplicate_system_function`, `fail/syntax_system_with_parameters` |
| `World.run` until a tick in which `World.stop` was called has finished | `world_run_stop`, `exit_code_zero` |
| `World.stop` has no effect outside `World.run` | `world_run_stop` |
| `World.tick` / `World.run` during a tick is a runtime error | `panic/world_*` |
| `World.tick` / `World.run` in the body of an `each` is a compile error, or a runtime error through a function | `fail/world_tick_in_each`, `panic/world_tick_in_each`, `panic/world_run_in_each` |
| `Time.tick`: ticks completed, `0` during the first tick, read-only | `time_fields`, `time_default_dt`, `fail/readonly_time_tick` |
| `Time.dt`: starts at `1.0 / 60`, writable | `time_default_dt`, `time_fields`, `system_first_program` |
| `Time.elapsed`: goes up by `Time.dt` at the end of each tick, read-only | `time_fields`, `time_default_dt`, `fail/readonly_time_elapsed` |

## GPU systems

The rules a `gpu system` must follow are in "What a GPU block may contain"
and "What a GPU system may touch" of the reference. Each row is a
rule and the programs that break it. A rule applies inside a GPU block and
inside every function one calls; the `gpu_function_*` programs break rules
there, and their diagnostics carry a note that names the system and the call.
Programs that follow every rule are checked by the unit tests in
`src/check/tests/gpu.rs`.

| Rule | Files |
| --- | --- |
| The body is one or more `each` blocks | `fail/gpu_no_each` |
| The body is `each` blocks and nothing else | `fail/gpu_statement_outside_each` |
| No output | `fail/gpu_output_puts`, `fail/gpu_output_print`, `fail/gpu_function_puts` |
| No structural change: `spawn`, `despawn`, `add`, `remove` | `fail/gpu_spawn`, `fail/gpu_despawn`, `fail/gpu_add`, `fail/gpu_remove`, `fail/gpu_function_transitive` |
| No `emit`, no `on` | `fail/gpu_emit`, `fail/gpu_on` |
| No strings: literals, interpolation, `to_s`, operands of a comparison | `fail/gpu_string_literal`, `fail/gpu_string_interpolation`, `fail/gpu_to_s`, `fail/gpu_string_compare`, `fail/gpu_function_local_string` |
| No lists: literals, iteration | `fail/gpu_list_literal`, `fail/gpu_list_iteration` |
| Locals are numbers, `Bool`s, enums, entities and plain records: no record with a string or a list at any depth | `fail/gpu_struct_with_string`, `fail/gpu_struct_with_list_nested` |
| No `e[C]`, read or written | `fail/gpu_index_component`, `fail/gpu_index_component_write` |
| No `has?`, no `alive?` | `fail/gpu_has`, `fail/gpu_alive` |
| No `World.count`, with a query or without | `fail/gpu_world_count`, `fail/gpu_function_world_count` |
| No `World.tick`, `World.run`, `World.stop` | `fail/gpu_world_control` |
| No `srand`, `clock`, `sleep`, `exit` | `fail/gpu_srand`, `fail/gpu_clock`, `fail/gpu_sleep`, `fail/gpu_exit` |
| No `break` directly in a GPU block; `break` in a loop inside it is fine | `fail/gpu_break`, `fail/gpu_break_modifier` |
| No `return` anywhere in a GPU block | `fail/gpu_return`, `fail/gpu_return_in_nested` |
| Resources and `Time` are read-only | `fail/gpu_write_resource`, `fail/gpu_write_resource_field`, `fail/gpu_write_time_dt`, `fail/gpu_function_write_resource` |
| The parameters of the nested `each` are read-only | `fail/gpu_write_nested_param`, `fail/gpu_write_nested_param_whole` |
| `Entity` parameters are read-only | `fail/gpu_write_entity_param` |
| One nested `each`: not a second level, not a second one in the block | `fail/gpu_each_two_deep`, `fail/gpu_each_second` |
| The nested `each` is directly inside the block: not in a loop, not in a branch | `fail/gpu_each_in_loop`, `fail/gpu_each_in_branch` |
| A query term is plain: no string or list field | `fail/gpu_term_string_field`, `fail/gpu_term_list_field` |
| A query term has no `Float` or `Int` field, at any depth; the hint names the fields and the type to declare | `fail/gpu_term_float_field`, `fail/gpu_term_int_field`, `fail/gpu_term_struct_field` |
| A component with a `UInt32` field is a query term, of the outer and of the nested `each`; the field itself is not read or assigned through the parameter | `gpu_uint32_term`, `fail/gpu_uint32_read_term_field`, `fail/gpu_uint32_assign_term_field`; the unit test `a_component_with_a_uint32_field_is_a_term` in `src/check/tests/gpu.rs` |
| The terms of the nested `each` follow the same rules, also when only read | `fail/gpu_term_nested_each` |
| Types named in `with:` and `without:` are not restricted | `fail/gpu_term_filter_unrestricted` |
| Resource fields that are read are numbers, `Bool`s, enums, entities or plain structs | `fail/gpu_resource_string`, `fail/gpu_resource_list`, `fail/gpu_resource_struct_with_list` |
| Constants that are read are numbers, `Bool`s, enums, entities or plain structs | `fail/gpu_constant_string`, `fail/gpu_constant_list` |
| A `UInt32` field itself is not used in GPU code: no read of one (a resource field, a field of a struct in a resource field or in a constant, the result of a call, a receiver's field in a method), no assignment to one with `=` or `+=`, no constructor of a struct that has one among its own fields (also as a field default, also in a called function); the hint is to declare the field `Int32` | `fail/gpu_resource_uint32`, `fail/gpu_function_resource_uint32`, `fail/gpu_uint32_read_field`, `fail/gpu_uint32_read_term_field`, `fail/gpu_uint32_read_constant_field`, `fail/gpu_uint32_read_call_result`, `fail/gpu_function_uint32_read_field`, `fail/gpu_uint32_assign`, `fail/gpu_uint32_assign_term_field`, `fail/gpu_uint32_compound_assign`, `fail/gpu_uint32_construct`, `fail/gpu_uint32_construct_default`, `fail/gpu_function_uint32_construct` |
| A struct that has a `UInt32` field is a value in GPU code: read from a constant and a resource field, copied, passed, returned, compared, put inside another struct, its other fields read and assigned, its method called; the other fields of a resource that has a `UInt32` field are read; a function that uses one stays legal for ordinary callers | `gpu_uint32_carried`, `gpu_uint32_beside`, `fail/gpu_uint32_read_field`, `fail/gpu_uint32_assign`, `fail/gpu_uint32_construct_default`, `fail/gpu_function_uint32_construct`, `fail/gpu_function_uint32_read_field`; the unit test `a_struct_with_a_uint32_field_is_a_gpu_value` in `src/check/tests/gpu.rs` |
| A called function takes and returns numbers, `Bool`s, enums, entities and plain records | `fail/gpu_call_returns_string`, `fail/gpu_call_takes_list`, `fail/gpu_call_method_receiver`, `fail/gpu_function_bad_call` |
| A called function does not recurse: directly, through another function, through an operator, or further down the calls | `fail/gpu_recursion_direct`, `fail/gpu_recursion_mutual`, `fail/gpu_recursion_operator`, `fail/gpu_recursion_reached` |
| A called function contains no query | `fail/gpu_function_each`, `fail/gpu_function_world_count` |
| A function is reported on once, however many calls and systems reach it | `fail/gpu_function_puts` |
| An `Int` literal fits in 32 bits, in a block, in a called function and in a field default a constructor uses | `fail/gpu_int_literal_too_big`, `fail/gpu_int_literal_too_small`, `fail/gpu_function_int_literal`, `fail/gpu_int_literal_default` |
| An `Int` constant that GPU code reads fits in 32 bits: above, below, in a called function, in a field default a constructor uses; a constant made from one that does not fit is judged by its own value | `fail/gpu_constant_too_big`, `fail/gpu_constant_too_small`, `fail/gpu_function_constant_too_big`, `fail/gpu_constant_default_too_big`, `gpu_constant_divisors`; the unit test `a_known_constant_fits_in_32_bits_where_gpu_code_reads_it` in `src/check/tests/gpu.rs` |
| Independent errors in one block are all reported | `fail/gpu_several` |

## What GPU systems compute

Programs with GPU systems that follow every rule. The suite runs them on the
C implementation of each block, and in its GPU mode on a device, so those
that print a `Float` a device may round differently state a tolerance in
their first line and print it rounded. Each expected output was
worked out by hand from "GPU systems" in the reference, except where
hand arithmetic does not reach: the orbits of `gpu_gravity`, the `Math`
results of `gpu_numbers`, and the random numbers of `gpu_rand`,
`gpu_control_flow` and `gpu_statements` were compared with separate C
programs written from the specification.

| Behaviour | Files |
| --- | --- |
| A block with no nested `each` updates its entities in place | `gpu_block_plain` |
| The gravity system of the specification | `gpu_gravity` |
| The nested `each` sees every entity as it was when the outer `each` began | `gpu_snapshot`, `gpu_tables`, `gpu_with_cpu_systems` |
| A run sees its own writes through the outer parameter and its old values through the nested one | `gpu_snapshot_own_row` |
| The outer and the nested query each match several tables | `gpu_tables` |
| `with:` and `without:`, one type or a list, on both queries; a filter may name a component with a `String` | `gpu_filters` |
| `Entity` terms: `==`, `!=`, `Entity.none`, a handle stored in a component | `gpu_entity_term`, `gpu_index_from_cpu`, `gpu_field_types` |
| `next` in the block ends the run and keeps its writes; a field whose assignment was not reached keeps its value | `gpu_next_keeps_writes` |
| `break` and `next` in the nested `each`, which visits entities in the order an ordinary `each` does | `gpu_nested_break`, `gpu_entity_term` |
| `break` and `next` in a loop inside the block | `gpu_next_keeps_writes`, `gpu_control_flow` |
| The `each` blocks of one system run in order and see earlier writes | `gpu_two_blocks`, `gpu_nested_break`, `gpu_filters` |
| Fields read and written: `Float32`, `Int8`, `Int16`, `Int32`, `UInt8`, `UInt16`, `Bool`, enum, `Entity`, a struct whole and by field | `gpu_field_types`, `gpu_struct_methods` |
| A component a block only reads can be 1, 3 or 6 bytes; every position of a field within a word | `gpu_odd_row_size` |
| Resource fields of every plain kind, known and unknown constants, struct constants | `gpu_uniforms`, `gpu_functions` |
| `Time.tick`, `Time.dt`, `Time.elapsed` | `gpu_uniforms`, `gpu_gravity` |
| Resources and `Time` are read when the `each` begins; a change between ticks is seen | `gpu_uniforms` |
| The largest and the smallest `Int` that fit in 32 bits come in whole from a resource field and from a constant; a `Float` is rounded | `gpu_uniforms` |
| An `Int` from outside that does not fit stops the program when the `each` begins: a resource field, a field of a struct in a resource field, a field of a struct constant; with no entity and no run that uses it | `panic/gpu_resource_int_too_big`, `panic/gpu_resource_int_too_small`, `panic/gpu_constant_int_too_big`, `panic/gpu_resource_int_too_big_unread`; the unit test `an_int_from_outside_is_checked_when_the_words_are_filled` in `src/emit/tests/gpu.rs` |
| A `UInt32` field beside a GPU block: the block reads the other fields of its resource and filters by a component that has one, and ordinary code reads and writes it between runs | `gpu_uint32_beside` |
| A struct with a `UInt32` field carried through a GPU block: values above the largest 32-bit `Int` come back unchanged and decide `==`, on the CPU and on a device | `gpu_uint32_carried` |
| A component with `UInt32` fields as a term: the fields the block does not name keep their values, a struct that holds one is copied from field to field and compared across a nested `each` and between components, and what ordinary code changes between runs is seen | `gpu_uint32_term`; the unit test `a_uint32_field_of_a_term_is_carried_as_its_bits` in `src/emit/tests/gpu.rs` |
| `Int` is 32 bits and wraps: `+`, `-`, `*`, unary minus, `abs`, `**` | `gpu_int_wrap`, `gpu_field_types` |
| `Float` is 32 bits: literals out of range, products that overflow or vanish, 16777217 | `gpu_float_literals`, `gpu_functions` |
| `to_i`, `floor`, `ceil`, `round`: saturation, 0 for NaN, halves away from zero; `nan?` | `gpu_float_to_int` |
| `Int` `/` floors and `%` takes the divisor's sign, by a variable and by a constant; the smallest `Int` by -1 | `gpu_div_mod` |
| `/` and `%` by constants made by `/`, `%` and `**`: a constant is worked out at 64 bits and fits in 32 where the block reads it; the same operators between constants in the block work at 32 bits | `gpu_constant_divisors` |
| `Float` `%` takes the divisor's sign | `gpu_div_mod` |
| `abs`, `clamp`, `min`, `max`, `to_f`, `round(digits)`, the `Math` functions, `**` on `Float` with either sign of base | `gpu_numbers`, `gpu_float_literals` |
| `**` on `Float`: a zero exponent, a zero base with an exponent of either sign, a negative base with a whole, a fractional and an infinite exponent | `gpu_float_power` |
| `%` on `Float` by zero is NaN | `gpu_float_power` |
| `%` on `Float` where rounding to 32 bits shows (`1000000.3125 % 0.1`), and beyond a quotient of 16,777,216 | not covered: the CPU and a GPU are allowed different results |
| `Math::PI` and `Math::E` | `gpu_replace_component` |
| A function computes at 32 bits when a GPU block calls it and at 64 bits otherwise | `gpu_functions`, `gpu_float_literals`, `gpu_int_wrap` |
| Functions that call functions, return early, loop, return nothing, return a struct | `gpu_functions`, `gpu_statements` |
| Struct construction, methods, operator methods, unary minus, `+=` through an operator, `==`, a struct with `Float` and `Int` fields | `gpu_struct_methods` |
| A whole component assigned through the parameter: from a constructor, with fields left to their defaults, from a function that takes and returns the component and reads `Time` | `gpu_replace_component` |
| `if`, `elsif`, `unless`, modifiers, `case` on `Int`, `Bool` and enum, the conditional operator, as statements and as values | `gpu_control_flow`, `gpu_statements`, `gpu_field_types` |
| `while`, `until`, `loop`, `times`, `upto`, `downto` | `gpu_control_flow`, `gpu_next_keeps_writes`, `gpu_functions` |
| `&&` and `||` evaluate their right side only when it decides the result | `gpu_control_flow` |
| A division that the conditional operator, `&&` or `||` skips does not fail | `gpu_statements` |
| Operands are evaluated left to right, also when a later one assigns to a variable an earlier one read | `gpu_statements` |
| `rand` and `rand(n)`: the same in every run; different per entity, tick, draw and block; draws in called functions count | `gpu_rand`, `gpu_control_flow` |
| `srand` changes the numbers; drawing in a GPU system does not move the ordinary generator | `gpu_rand` |
| A call whose value is unused still draws | `gpu_statements` |
| `rand` follows the hash chain of the reference | `gpu_rand` (its numbers were compared with a separate C program) |
| A comparison with NaN, and a `Math` function outside its domain, on a GPU | not covered: the result is not specified |
| A block that does not fit the device runs on the CPU | not covered: it needs a device with small limits |
| `World.tick` returns when the GPU has finished the tick's work | not observable from a program's output; not covered |
| A GPU system between ordinary systems that read and write the same components, and top-level code between ticks | `gpu_with_cpu_systems`, `gpu_index_from_cpu` |
| On a device, the uploads and downloads `MIDNIGHT_GPU=stats` counts in a tick are the ones `midnight plan` lists; a column no ordinary code uses moves once | `gpu_transfers` |
| `e[C]` in ordinary code on a component a GPU block names, read, written and replaced | `gpu_index_from_cpu`, `gpu_entity_term` |
| A block that uses none of its parameters | `gpu_statements` |
| Names that WGSL or C reserve, as fields, locals, parameters, functions and types | `gpu_names` |
| Run-time errors in a GPU system | `panic/gpu_*` (see [Runtime errors](#runtime-errors)) |

## Events

| Rule | Files |
| --- | --- |
| `emit` takes effect at once, inside `each` too | `event_emit_in_each`, `event_combat` |
| `on` runs once per unseen event, oldest first; the parameter is a read-only copy | `event_each_on_once`, `event_types_independent`, `break_on`, `fail/readonly_on_param` |
| Every `on` keeps its own position | `event_each_on_once`, `break_on` |
| An `on` after the `emit` sees it in the same tick; one before it sees it in the next tick | `event_same_tick_next_tick` |
| Discarded at the end of the tick after the one it was emitted in | `event_discard`, `event_top_level` |
| Events emitted from top-level code count as emitted in the next tick | `event_top_level` |
| Events emitted while an `on` block runs are left for the next time that `on` runs | `event_emitted_during_on` |
| `break` skips the rest of the current batch | `event_break_next` (assumption 3) |
| `on` runs when control reaches it: at the top level, in functions | `event_top_level`, `event_each_on_once`, `fn_world_access` (assumption 4) |
| Event types are independent; events carry strings, lists, structs, enums, handles | `event_types_independent`, `event_combat`, `refcount` |
| `emit` / `on` with a non-event | `fail/emit_non_event`, `fail/on_non_event` |

## Built-ins

### Output

| Construct | Files |
| --- | --- |
| `puts a, b`; `puts` alone | `out_puts` |
| `print` | `out_print` |
| `puts` of a string ending in a newline | `out_puts_trailing_newline` (assumption 8) |
| `to_s` of `Int` | `tos_int` |
| `to_s` of `Float`: `1.0`, `0.1`, `1e+20`, positional range | `tos_float`, `tos_float_range`, `types_basic` |
| `to_s` of `Infinity`, `-Infinity`, `NaN` | `tos_float_special`, `ops_float_special` |
| `to_s` of `Bool`, `String`, enum | `tos_bool_string_enum` |
| `to_s` of `Entity` | `entity_to_s` |
| `to_s` of struct, component, event | `tos_struct`, `struct_equality_to_s`, `out_puts_types` |
| `to_s` of lists, nested and empty | `tos_list` |
| A string inside a struct or list is quoted, with escapes | `tos_string_escapes`, `struct_equality_to_s` |

### Numbers

| Expression | Files |
| --- | --- |
| `n.abs`, `x.abs` | `num_abs`, `ops_unary_minus` |
| `n.to_f` | `num_conversions` |
| `x.to_i` | `num_conversions` |
| `x.floor`, `x.ceil`, `x.round` | `num_conversions`, `num_round_half` (assumption 11) |
| `x.round(digits)` | `num_round_digits` |
| `x.nan?` | `num_nan`, `ops_float_special` |
| `v.clamp(lo, hi)` for Int and Float | `num_clamp` |
| `min(a, b)`, `max(a, b)` | `num_min_max` |
| `Math.sqrt`, `sin`, `cos`, `tan`, `atan2`, `exp`, `log` | `math_functions` |
| `Math::PI`, `Math::E` | `math_constants`, `const_expressions` |

### Strings

| Expression | Files |
| --- | --- |
| `s.length`, `s.bytesize` | `str_length_bytesize`, `str_escape_lengths` |
| `s.empty?` | `str_predicates` |
| `s.upcase`, `s.downcase` | `str_case` |
| `s.include?(t)`, `s.start_with?(t)`, `s.end_with?(t)` | `str_predicates` |
| `s.ljust(n)`, `s.rjust(n)` | `str_justify` |
| `s.to_i`, `s.to_f` | `str_to_number` |
| `s + t`, `s * n` | `str_concat_repeat`, `ops_string` |

### Lists

| Expression | Files |
| --- | --- |
| `xs.length`, `xs.empty?` | `list_basics` |
| `xs[i]` with positive and negative indexes | `list_basics` |
| `xs[i] = v` with positive and negative indexes | `list_index_write` |
| `xs.first`, `xs.last` | `list_basics` |
| `xs.push(v)` | `list_push_pop` |
| `xs.pop` | `list_push_pop` |
| `xs.delete_at(i)` | `list_delete_at`, `list_delete_at_negative` (assumption 12) |
| `xs.clear` | `list_clear` |
| `xs.include?(v)` | `list_include` |
| `xs + ys`, `xs * n` | `ops_list` |
| `xs.each`, `xs.each_with_index` | `loop_list_each`, `loop_each_with_index` |
| Element types of a literal; an Int among Floats | `lex_list_literals`, `conv_int_to_float`, `fail/type_list_elements` |
| `[]` takes its type from an annotation, a field, an argument, a declared return type | `lex_list_literals`, `vars_creation`, `fn_empty_list_return`, `fail/empty_list_no_type`, `fail/empty_list_no_type_method` |
| Mutation needs a list that is a place | `fail/push_readonly_list`, `fail/pop_readonly_list`, `fail/index_assign_readonly_param`, `fail/push_readonly_constant`, `fail/clear_readonly_constant`, `fail/delete_at_readonly_on_param`, `fail/push_readonly_iteration_param`, `fail/push_not_a_place` |
| Nested lists and lists of structs modified in place | `value_copy_lists`, `struct_fields_lists`, `loop_update_by_index`, `refcount` |

### Random numbers, time and exit

| Expression | Files |
| --- | --- |
| `rand` | `rand_properties` |
| `rand(n)` | `rand_properties`, `panic/rand_zero`, `panic/rand_negative` |
| `srand(seed)` | `rand_srand` |
| `srand` | `rand_srand` |
| `clock` | `clock_sleep` |
| `sleep(secs)` | `clock_sleep` |
| `exit`, `exit(code)` (status 0 only) | `exit_bare`, `exit_code_zero` |
| The generator starts from the same seed in every run | not covered: it needs two runs of one program |
| `exit` with a non-zero status | not covered: `tests/run` requires status 0 and `tests/panic` a runtime error message |

## Programs and files

| Rule | Files |
| --- | --- |
| Declarations visible before and after their definition, in every file | `fn_use_before_definition`, `system_order`, `require_declarations` |
| Top-level statements run once, in order | every program; `require_order` |
| Top-level variables are invisible in functions and systems | `fail/toplevel_variable_in_function`, `fail/toplevel_variable_in_system` |
| `require` loads a file relative to the requiring file | `require_declarations`, `require_order` (`lib/outer` requires `inner`) |
| `require` at the top level only; a file that can't be found | `fail/require_not_top_level`, `fail/require_not_found` |
| A file is loaded at most once | `require_order`, `require_systems` |
| Its top-level statements run at the point of the first `require` | `require_order` |
| Its systems are ordered where the `require` appears | `require_systems` |

## What things cost

| Fact | Files |
| --- | --- |
| A component's fields are ordered to minimise padding; `Bool` and enum fields take one byte | `layout` (the size is asserted by a separate harness step) |
| A component a GPU block writes is rounded up to a multiple of 4 bytes; one that GPU blocks only read keeps its size | `gpu_odd_row_size`, `gpu_field_types` (the generated C asserts the size and every offset of each component a GPU block names); the unit tests `a_written_component_is_whole_words` and `a_read_only_column_can_have_rows_of_any_size` in `src/emit/tests/gpu.rs` |
| What a GPU system costs: the fixed cost of a block, the cost of a transfer, the copy a nested `each` reads | not observable from a program's output; `nix run .#bench-gpu` measures the first two |
| The other layout and cost facts (tables, 20 bytes per entity, tags take no space, costs of operations) | not observable from a program's output; not covered |

## Errors

| Rule | Files |
| --- | --- |
| Compile-time errors name the file, line and column and suggest a fix | every `tests/fail` program; the `*_suggest` files for suggestions |
| Several independent errors are all reported | `fail/three_errors` |
| At most 20 errors are reported | `fail/error_cap` |
| Runtime errors print `FILE:LINE: runtime error: MESSAGE` and exit with status 1 | every `tests/panic` program |

## Grammar

Every production of the grammar is used by at least one `tests/run` program.
Syntax errors are in `fail/syntax_*`: `syntax_unexpected_token`,
`syntax_stray_end`, `syntax_unclosed_if`, `syntax_unclosed_def`,
`syntax_unclosed_do`, `syntax_unclosed_struct`, `syntax_unclosed_while`,
`syntax_unclosed_system`, `syntax_assignment_as_expression`,
`syntax_call_without_parentheses`, `syntax_keyword_as_variable`,
`syntax_gpu_reserved`, `syntax_lowercase_type_name`,
`syntax_param_without_type`, `syntax_unless_elsif`,
`syntax_system_with_parameters`, `syntax_resource_method`,
`syntax_unterminated_string`, `syntax_variable_question_mark`.

## The two special tests

| Test | Covers |
| --- | --- |
| `refcount` | Strings and lists copied, modified and dropped through locals, struct fields, component fields, resource fields, events (read and discarded unread), queued commands (spawn and add inside `each`, also for entities despawned in the same batch), despawned entities, lists of strings, of lists and of structs with strings, arguments and return values, early `return` / `break` / `next` with live locals, replaced components (`add` of a present one, `e[C] = …`), removed components, reassigned variables, `pop` / `delete_at` / `clear` |
| `layout` | A component with a `Float`, a `Float32`, an `Int16`, a `UInt8`, a `Bool` and an enum field (`Body`, with the enum `Kind`), spawned, queried, modified and printed |

## Transfer plans

What `midnight plan` prints. Each expected plan was worked out by hand from
the rule for where a column lives.

| Case | Files |
| --- | --- |
| Data that only GPU systems use stays on the GPU: an empty plan | `plan/stays_on_gpu` |
| Top-level code before the loop that ticks is not part of a tick | `plan/stays_on_gpu`, `plan/top_level` |
| Top-level code after the loop that ticks is not part of a tick | `plan/after_ticks` |
| A CPU system that reads what a GPU system writes: one download, no upload | `plan/reader` |
| A component a GPU system only reads is never downloaded | `plan/reader`, `plan/writer`, `plan/nested_each` |
| A CPU system that writes what a GPU system reads: an upload | `plan/writer`, `plan/nested_each` |
| A CPU system that writes what a GPU system writes: an upload and a download | `plan/writer`, `plan/functions` |
| A download comes from the last GPU system to write; consecutive GPU systems transfer nothing | `plan/writer`, `plan/nested_each` |
| A nested `each` over a component its block writes is no transfer | `plan/nested_each` |
| A component named only by a nested `each` has a GPU copy | `plan/nested_each` |
| What called functions do counts as the system's | `plan/functions` |
| `e[C]`, read and written | `plan/functions`, `plan/top_level` |
| `World.count` and `has?` read no column | `plan/functions` |
| `spawn` changes the components it lists: `on spawn` | `plan/spawn` |
| `despawn` changes every component with a GPU copy: `on despawn` | `plan/despawn` |
| `add` and `remove` change every component with a GPU copy: `on add`, `on remove` | `plan/add_remove` |
| A marked download before an unmarked one of the same component: both are listed | `plan/despawn` |
| The plan shown in the reference | `plan/despawn` |
| `with:` and `without:` filters read no column | `plan/add_remove` (`freeze` filters on Heat, and its download is marked `on add`) |
| The mark names the first structural change of a system that touches the component | the unit test `the_first_structural_change_of_a_system_is_named` in `src/plan/tests.rs` |
| Ticks written out one after another, with code between them, are not followed | the unit test `top_level_code_between_ticks_outside_a_loop_does_not_count` in `src/plan/tests.rs` |
| A structural change that finds the CPU copy current downloads nothing | `plan/add_remove` |
| Components with no GPU copy never appear | `plan/despawn`, `plan/spawn` |
| Top-level code in the loop that ticks is the step `top level` | `plan/top_level` |
| Columns are lined up within a plan | `plan/despawn`, `plan/add_remove`, `plan/writer` |

## Runtime errors

| Runtime error | Variant | File | Expected message |
| --- | --- | --- | --- |
| `e[C]`, entity alive but lacks `C` | read | `panic/component_missing_read` | `Entity(0v1) has no Health` |
| | field write | `panic/component_missing_write` | `Entity(0v1) has no Health` |
| | whole replace | `panic/component_missing_replace` | `Entity(0v1) has no Health` |
| | inside a function | `panic/component_missing_in_function` | `Entity(0v1) has no Health` |
| | after `remove` | `panic/component_removed_read` | `Entity(0v1) has no Health` |
| `e[C]`, entity dead | read | `panic/component_dead_read` | `Entity(0v1) is not alive` |
| | field write | `panic/component_dead_write` | `Entity(0v1) is not alive` |
| | whole replace | `panic/component_dead_replace` | `Entity(0v1) is not alive` |
| | old handle after the slot may have been reused | `panic/component_stale_handle` | `Entity(0v1) is not alive` |
| `e[C]` on `Entity.none` | read | `panic/component_none_read` | `Entity(none) is not alive` |
| | write | `panic/component_none_write` | `Entity(none) is not alive` |
| List index out of range | read | `panic/list_index_read` | `index 5 out of range for a list of length 3` |
| | read at the length | `panic/list_index_read_at_length` | `index 3 out of range for a list of length 3` |
| | write | `panic/list_index_write` | `index 5 out of range for a list of length 3` |
| | compound write | `panic/list_index_compound` | `index 3 out of range for a list of length 3` |
| | negative, read | `panic/list_index_negative_read` | `index -4 out of range for a list of length 3` |
| | negative, write | `panic/list_index_negative_write` | `index -4 out of range for a list of length 3` |
| | empty list | `panic/list_index_empty` | `index 0 out of range for a list of length 0` |
| | inner list | `panic/list_index_nested` | `index 1 out of range for a list of length 1` |
| | `delete_at` | `panic/list_delete_at` | `index 3 out of range for a list of length 3` |
| | `delete_at`, negative | `panic/list_delete_at_negative` | `index -4 out of range for a list of length 3` |
| `pop` / `first` / `last` on an empty list | `pop` | `panic/list_pop_empty` | `pop on an empty list` |
| | `first` | `panic/list_first_empty` | `first on an empty list` |
| | `last` | `panic/list_last_empty` | `last on an empty list` |
| Int `/` by zero | `/` | `panic/int_division_by_zero` | `division by zero` |
| | `/=` | `panic/int_division_assign_by_zero` | `division by zero` |
| | in a function | `panic/int_division_by_zero_in_function` | `division by zero` |
| Int `%` by zero | `%` | `panic/int_modulo_by_zero` | `modulo by zero` |
| | `%=` | `panic/int_modulo_assign_by_zero` | `modulo by zero` |
| Int does not fit a compact field or element | `Int8` above | `panic/compact_int8_overflow` | `128 does not fit in Int8` |
| | `Int8` below | `panic/compact_int8_underflow` | `-129 does not fit in Int8` |
| | `Int16` above, constructor argument | `panic/compact_int16_constructor` | `32768 does not fit in Int16` |
| | `Int16` below | `panic/compact_int16_underflow` | `-32769 does not fit in Int16` |
| | `Int16`, component given to `spawn` | `panic/compact_spawn_component` | `40000 does not fit in Int16` |
| | `Int32` above | `panic/compact_int32_overflow` | `2147483648 does not fit in Int32` |
| | `Int32` below | `panic/compact_int32_underflow` | `-2147483649 does not fit in Int32` |
| | `UInt8` above | `panic/compact_uint8_overflow` | `300 does not fit in UInt8` |
| | `UInt8` negative | `panic/compact_uint8_negative` | `-1 does not fit in UInt8` |
| | `UInt8`, compound assignment on `e[C]` | `panic/compact_uint8_compound` | `260 does not fit in UInt8` |
| | `UInt8`, through a query parameter | `panic/compact_uint8_query_param` | `-100 does not fit in UInt8` |
| | `UInt8`, resource field | `panic/compact_resource_field` | `256 does not fit in UInt8` |
| | `UInt8`, event field | `panic/compact_event_field` | `999 does not fit in UInt8` |
| | `UInt16`, keyword constructor argument | `panic/compact_uint16_constructor_keyword` | `65536 does not fit in UInt16` |
| | `UInt32` above | `panic/compact_uint32_overflow` | `4294967296 does not fit in UInt32` |
| | `UInt32` negative, compound assignment | `panic/compact_uint32_negative` | `-5 does not fit in UInt32` |
| | list element store | `panic/compact_list_element` | `256 does not fit in UInt8` |
| | list element compound store | `panic/compact_list_element_compound` | `260 does not fit in UInt8` |
| | `push` onto a `[Int8]` | `panic/compact_list_push` | `200 does not fit in Int8` |
| | `push` onto a `[UInt16]` component field | `panic/compact_list_field_push` | `70000 does not fit in UInt16` |
| | list literal with a computed element | `panic/compact_list_literal` | `300 does not fit in UInt8` |
| Int `**` with a negative exponent | variable | `panic/power_negative_exponent` | `negative exponent -2` |
| | computed | `panic/power_negative_exponent_computed` | `negative exponent -3` |
| `rand(n)` with `n <= 0` | zero | `panic/rand_zero` | `rand(0): the argument must be positive` |
| | zero from a list length | `panic/rand_empty_list_length` | `rand(0): the argument must be positive` |
| | negative | `panic/rand_negative` | `rand(-3): the argument must be positive` |
| `World.tick` during a tick | in a system, under `World.tick` | `panic/world_tick_in_system` | `World.tick called while a tick is in progress` |
| | in a system, under `World.run` | `panic/world_tick_in_system_during_run` | `World.tick called while a tick is in progress` |
| | in a function called by a system | `panic/world_tick_in_function` | `World.tick called while a tick is in progress` |
| `World.run` during a tick | in a system, under `World.tick` | `panic/world_run_in_system` | `World.run called while a tick is in progress` |
| | in a system, under `World.run` | `panic/world_run_in_system_during_run` | `World.run called while a tick is in progress` |
| `World.tick` inside an `each` | in a function called from the body | `panic/world_tick_in_each` | `World.tick called inside an `each`` |
| `World.run` inside an `each` | in a function called from the body | `panic/world_run_in_each` | `World.run called inside an `each`` |
| A failure in a GPU system (the message names no value) | `Int` `/` by zero | `panic/gpu_division_by_zero` | `division by zero` |
| | `Int` `%` by zero, in the nested `each` | `panic/gpu_modulo_by_zero` | `modulo by zero` |
| | `Int` `**` with a negative exponent | `panic/gpu_negative_exponent` | `negative exponent` |
| | `rand(n)` with `n <= 0` | `panic/gpu_rand_not_positive` | `rand: the argument must be positive` |
| | `UInt8` above, compound assignment | `panic/gpu_store_uint8` | `value does not fit in UInt8` |
| | `Int8` below | `panic/gpu_store_int8` | `value does not fit in Int8` |
| | `Int16` above, constructor argument | `panic/gpu_store_int16` | `value does not fit in Int16` |
| | `UInt16` negative, field of a struct in a component | `panic/gpu_store_uint16` | `value does not fit in UInt16` |
| | in a function the block calls: the line in the function | `panic/gpu_failure_in_function` | `modulo by zero` |
| | two operations fail in different runs: the earliest in the source is reported | `panic/gpu_earliest_failure` | `division by zero` |
| | operations fail in two files: the one in the file loaded first is reported | not covered | |
| | a failed operation gives 0 and the run goes on; a failed `rand(n)` draws no number | not observable: the program stops | |
| | nothing after the failing `each` runs | `panic/gpu_failure_stops_tick` | `negative exponent` |
| An `Int` from outside that does not fit, in a GPU system (reported when the `each` begins, on its line) | resource field, one above the largest, after a tick in which it fitted | `panic/gpu_resource_int_too_big` | `Budget.limit is 2147483648, which does not fit in the 32-bit Int of a GPU system` |
| | field of a struct in a resource field, one below the smallest; the block reads another field | `panic/gpu_resource_int_too_small` | `Board.grid.origin is -2147483649, which does not fit in the 32-bit Int of a GPU system` |
| | field of a struct constant | `panic/gpu_constant_int_too_big` | `LIMITS.high is 5000000000, which does not fit in the 32-bit Int of a GPU system` |
| | no entity matches, and the read is in a function behind a test | `panic/gpu_resource_int_too_big_unread` | `Budget.limit is 4294967296, which does not fit in the 32-bit Int of a GPU system` |

## Assumptions

Readings the tests rely on where the reference is ambiguous or silent. Each
is kept to the files named, so a disagreement stays local.

1. Entity numbering: indexes from 0, generations from 1, a freed slot reused
   by the next spawn with generation 2; a dead handle still prints its own
   index and generation. `entity_to_s`, and the messages of
   `panic/component_*`.
2. Entities with one component set, spawned in sequence with no removals,
   are visited in spawn order. `query_order_spawn` only.
3. `break` in `on` consumes the rest of the batch: the skipped events are
   not delivered to that `on` later. `event_break_next`.
4. A top-level `on` (or one in a function called from the top level) sees
   events emitted earlier from top-level code, with no tick run.
   `event_top_level`, `each_only_queries_defer`, `event_emit_in_each`,
   `break_on`, `modifier_break_next_emit`, `fn_world_access`.
5. A list literal of only Ints converts where a `[Float]` is expected.
   `conv_list_literal`.
6. Float `%` is floored like Int `%` (`-7.5 % 2.0` is `0.5`).
   `ops_float_modulo`.
7. Strings order bytewise (`"Zebra" < "apple"`). `ops_compare_strings`.
8. `puts "a\n"` writes two newlines. `out_puts_trailing_newline`.
9. `Int ** Int` wraps on overflow. `types_int_wrap_power`.
10. The smallest Int divided by -1 wraps, and its remainder is 0.
    `types_int_wrap_division`.
11. `round` sends halves away from zero. `num_round_half`.
12. `delete_at` accepts a negative index. `list_delete_at_negative`.
13. Mixed Int and Float operands are accepted by `<`, `<=`, `>`, `>=`, and
    Int arguments by `clamp` on a Float and by `include?` on a `[Float]`.
    `ops_int_float_mixing`, `num_clamp`, `conv_list_elements`.
14. Quoted strings inside lists and structs escape `"`, `\`, newline and tab
    as `\"`, `\\`, `\n`, `\t`. Other characters are not tested.
    `tos_string_escapes`, `struct_equality_to_s`.
15. `to_s` and positional construction use the declaration order of fields,
    whatever order they are stored in. Every test that prints a struct;
    most directly `layout`.
16. A bare component name is accepted by `add` when every field has a
    default, and replaces a component that is present with its defaults.
    `entity_add_bare`.
17. A queued `add` carries the value its argument had when the `add` ran,
    and replaces a present component when the queue is applied, overwriting
    field writes made in between. `each_deferred_add_remove`.
18. Queued changes aimed at an entity that an earlier queued `despawn`
    killed do nothing. `each_deferred_order`, `refcount`.
19. `[]` takes its type from a declared return type, from the variable it is
    assigned to, and from a parameter. `fn_empty_list_return`,
    `vars_creation`, `lex_list_literals`.
20. A multi-line `case` can be written inside a call's parentheses.
    `case_expression_argument`.
21. A `case` over an enum that covers every variant is a value without an
    `else`. `case_enum_exhaustive`, `enum_case_value`.
22. Keyword arguments are evaluated in the order written.
    `eval_keyword_args`.
23. `return` inside list iteration, `loop do` and `while` leaves the
    enclosing function. `modifier_return`, `loop_loop`, `fn_early_return`,
    `refcount`.
24. Structural changes made outside a query, including inside list
    iteration, counting loops and `on`, are immediate.
    `each_only_queries_defer`.
25. `World.stop` called during a plain `World.tick` does not affect a later
    `World.run`. `world_run_stop`.
26. A system that changes `Time.dt` changes the amount added to
    `Time.elapsed` at the end of that same tick. `time_fields`.
27. A view parameter of `each` and `e[C]` for the same entity are the same
    storage: a write through one is read through the other at once.
    `each_field_writes_immediate`.
28. Constants are usable in a function defined above them, and a required
    file's constant after the `require` line. `const_scopes`,
    `require_declarations`.
29. A file reached as `lib/inner` from the root and as `inner` from
    `lib/outer` is one file. `require_order`.
30. `exit` with strings and lists still alive passes the debug build's
    check that buffers are freed. `exit_bare`, `exit_code_zero`.
31. Compound assignment works for every struct operator and for string and
    list `*=`. `struct_operators`, `ops_compound_assign`,
    `str_concat_repeat`.
32. `round(digits)` results print as the shortest text for the rounded
    value (`3.14`, `1234.6`). `num_round_digits`.
33. Inside an `each` with queued changes the tests never use `World.count`
    with no arguments or `each Entity`: the reference says a queued spawn is
    "alive" but that queries see the old world, and does not say which wins.

## Contradictions in the reference

1. `case` is a primary expression and so may appear inside parentheses,
   where newlines do not end statements, yet `case` needs newlines between
   its arms. `case_expression_argument`.
2. The prose says a zero-parameter function "is declared without
   parentheses"; the grammar also accepts `def f()`. Not tested.
3. The prose says `==` can't be redefined; the grammar's `OPERATOR` does not
   include `==`, so `def ==` is a syntax error first.
   `fail/struct_redefines_equality` accepts either diagnostic.
4. Comparisons "do not chain", and the parser rejects `1 < x < 3` itself,
   although the grammar as written would parse it. `fail/comparison_chain`.

## Diagnostics

One row per `tests/fail` program. Each program states what must be reported,
and where, in its `# expect:` and `# at:` header lines; the exact text is not
in the reference, so each `.err` holds the compiler's output and is reviewed
against that header.

| Diagnostic | File | Line | Reference section |
| --- | --- | --- | --- |
| the component `Health` can't be listed bare in `add`: its field `hp` has no default | `add_bare_no_defaults.12am` | 10 | Entities |
| `add` takes components and tags; `Vec2` is a struct | `add_non_component.12am` | 10 | Entities |
| wrong number of arguments: `min` takes 2, found 1 | `args_builtin_count.12am` | 4 | Functions; Structs: Construction |
| wrong number of arguments: `Vec2` has 2 fields, found 3 values | `args_constructor_count.12am` | 9 | Functions; Structs: Construction |
| wrong number of arguments: `double` takes 1, found 2 | `args_function_count.12am` | 8 | Functions; Structs: Construction |
| wrong number of arguments: `greet` takes none, found 1 | `args_function_no_params.12am` | 8 | Functions; Structs: Construction |
| wrong number of arguments: `add` takes 2, found 1 | `args_function_too_few.12am` | 8 | Functions; Structs: Construction |
| wrong number of arguments: the method `scaled` of `Rect` takes 1, found 2 | `args_method_count.12am` | 14 | Functions; Structs: Construction |
| missing field `y` in the constructor of `Vec2` (it has no default) | `args_missing_field.12am` | 9 | Functions; Structs: Construction |
| missing field `y` in the constructor of `Vec2` (it has no default, so it can't be left off the end) | `args_missing_field_positional.12am` | 9 | Functions; Structs: Construction |
| a constructor call can't mix positional and keyword arguments | `args_mixed_positional_keyword.12am` | 9 | Functions; Structs: Construction |
| the keyword argument `x` is given twice | `args_repeated_keyword.12am` | 9 | Functions; Structs: Construction |
| unknown keyword argument `depth`: `Rect` has no field of that name | `args_unknown_keyword.12am` | 9 | Functions; Structs: Construction |
| type mismatch: `n` is an `Int`, but `n + 0.5` is a `Float` (a `Float` does not convert to `Int`) | `assign_compound_float_to_int.12am` | 5 | Types; Expressions |
| type mismatch: `x` is an `Int` (from its first assignment on line 4), found a `String` | `assign_different_type.12am` | 5 | Types; Expressions |
| type mismatch: `n` is an `Int` (from its first assignment on line 4), found a `Float` (a `Float` does not convert to `Int`) | `assign_float_to_int_variable.12am` | 5 | Types; Expressions |
| the block parameter `item` is named twice (the second reuses a name already in scope) | `block_param_duplicate.12am` | 5 | Variables and values; Programs and files |
| the block parameter `i` reuses the name of a variable that is already in scope (created on line 4) | `block_param_in_scope.12am` | 5 | Variables and values; Programs and files |
| the block parameter `i` reuses the name of the enclosing block's parameter (line 4) | `block_param_nested_reuse.12am` | 5 | Variables and values; Programs and files |
| the block parameter `pos` reuses the name of a variable that is already in scope (created on line 9) | `block_param_query_reuse.12am` | 11 | Variables and values; Programs and files |
| the block parameter `n` reuses the name of the function parameter `n` | `block_param_reuses_function_param.12am` | 5 | Variables and values; Programs and files |
| `break` outside a loop | `break_outside_loop.12am` | 5 | Control flow |
| `UInt8` is a built-in name (a compact type) and can't be redefined | `builtin_compact_type_redefined.12am` | 4 | Lexical structure: built-in names |
| `World` is a built-in name and can't be redefined (here, as a constant) | `builtin_name_redefined_constant.12am` | 4 | Lexical structure: built-in names |
| `Entity` is a built-in name and can't be redefined | `builtin_type_redefined.12am` | 4 | Lexical structure: built-in names |
| the `case` over the enum `Team` has no `else` and does not cover every variant; the missing one is named: `Green` | `case_enum_missing_one.12am` | 11 | Control flow: case |
| the `case` over the enum `Team` has no `else` and does not cover every variant; the missing ones are named: `Blue` and `Green` | `case_enum_missing_variants.12am` | 11 | Control flow: case |
| a `case` value must be an `Int`, a `String`, a `Bool` or an enum; found a `Float` | `case_value_float.12am` | 5 | Control flow: case |
| a `case` value must be an `Int`, a `String`, a `Bool` or an enum; found a `Vec2` | `case_value_struct.12am` | 10 | Control flow: case |
| `clear` needs a list that can be modified; `PRIMES` is a constant | `clear_readonly_constant.12am` | 6 | What can be modified; Lists |
| the constant 40000 does not fit in `Int16` (the field `level` of `Stats`; -32768 to 32767) | `compact_assign_int16_too_big.12am` | 9 | Compact field types |
| the constant 256 does not fit in `UInt8` (the field `hp` of `Stats`) | `compact_assign_too_big.12am` | 9 | Compact field types |
| the constant 300 (the value of `200 + 100`) does not fit in `UInt8` | `compact_constant_expression_too_big.12am` | 5 | Compact field types |
| the constant -129 does not fit in `Int8` (the field `offset` of `Tile`; -128 to 127) | `compact_constructor_keyword_too_small.12am` | 8 | Compact field types |
| the constant 300 does not fit in `UInt8` (the field `hp` of `Health`) | `compact_constructor_too_big.12am` | 8 | Compact field types |
| the constant -1 does not fit in `UInt8` (0 to 255) | `compact_default_negative_unsigned.12am` | 5 | Compact field types |
| the constant 300 does not fit in `UInt8` (0 to 255) | `compact_default_too_big.12am` | 5 | Compact field types |
| the compact type `Float32` can't be the type of a variable (only of fields and list elements); use `Float` | `compact_float32_variable.12am` | 4 | Compact field types |
| the constant 2147483648 does not fit in `Int32` (the field `offset` of `Record`) | `compact_int32_too_big.12am` | 8 | Compact field types |
| the constant 256 does not fit in `UInt8` (an element of a `[UInt8]`) | `compact_list_element_assign_too_big.12am` | 5 | Compact field types |
| the constant 256 does not fit in `UInt8` (an element of a `[UInt8]`) | `compact_list_literal_too_big.12am` | 4 | Compact field types |
| the constant 300 does not fit in `UInt8` (an element of a `[UInt8]`) | `compact_list_push_too_big.12am` | 5 | Compact field types |
| the constant 300 (the value of `BIG`) does not fit in `UInt8` (the field `hp` of `Health`) | `compact_named_constant_too_big.12am` | 10 | Compact field types |
| the compact type `Int16` can't be the type of a parameter (only of fields and list elements); use `Int` | `compact_parameter.12am` | 4 | Compact field types |
| the compact type `UInt8` can't be a return type (only the type of fields and list elements); use `Int` | `compact_return.12am` | 4 | Compact field types |
| the constant 65536 does not fit in `UInt16` (0 to 65535) | `compact_uint16_too_big.12am` | 5 | Compact field types |
| the constant -1 does not fit in `UInt32` (the field `id` of `Record`) | `compact_uint32_negative.12am` | 9 | Compact field types |
| the compact type `UInt8` can't be the type of a variable (only of fields and list elements); use `Int` | `compact_variable.12am` | 4 | Compact field types |
| type mismatch: `<` works on numbers and strings, not on `Bool` | `compare_bools.12am` | 6 | Expressions: Comparison |
| type mismatch: `<` works on numbers and strings, not on the enum `Team` | `compare_enums.12am` | 9 | Expressions: Comparison |
| type mismatch: `<` works on numbers and strings, not on `Vec2` | `compare_structs.12am` | 11 | Expressions: Comparison |
| comparison operators do not chain (`1 < x` is a `Bool`, which can't be compared with `3`); write `1 < x && x < 3` | `comparison_chain.12am` | 5 | Expressions: Comparison |
| the component `Marker` has no fields (a component has at least one); hint: declare it with `tag Marker` | `component_no_fields.12am` | 4 | Components and tags |
| the condition of `elsif` must be a `Bool`, found a `String` | `cond_elsif_not_bool.12am` | 8 | Types: No truthiness |
| the condition of `if` must be a `Bool`, found an `Int` | `cond_if_not_bool.12am` | 5 | Types: No truthiness |
| the condition of the `if` modifier must be a `Bool`, found an `Int` | `cond_modifier_not_bool.12am` | 5 | Types: No truthiness |
| the condition of the `unless` modifier must be a `Bool`, found an `Entity` | `cond_modifier_unless_not_bool.12am` | 5 | Types: No truthiness |
| the condition of `? :` must be a `Bool`, found an `Int` | `cond_ternary_not_bool.12am` | 5 | Types: No truthiness |
| the condition of `unless` must be a `Bool`, found a `String` | `cond_unless_not_bool.12am` | 5 | Types: No truthiness |
| the condition of `until` must be a `Bool`, found a `[Int]` | `cond_until_not_bool.12am` | 5 | Types: No truthiness |
| the condition of `while` must be a `Bool`, found an `Int` | `cond_while_not_bool.12am` | 5 | Types: No truthiness |
| the constant `LIMIT` is already defined (on line 4) and can't be changed | `const_assigned_twice.12am` | 5 | Constants; Structs: Defaults |
| a constant can be defined only by a top-level assignment, not inside a function | `const_in_function.12am` | 5 | Constants; Structs: Defaults |
| the value of the constant `START` is not a constant expression: `rand` can't be called in one | `const_not_constant_expression.12am` | 4 | Constants; Structs: Defaults |
| the constant `BASE` is used before its definition on line 5 (a constant may use only constants defined above it) | `const_uses_later_constant.12am` | 4 | Constants; Structs: Defaults |
| the value of the constant `AREA` is not a constant expression: it uses the variable `size` | `const_uses_variable.12am` | 5 | Constants; Structs: Defaults |
| a term of `World.count` must be a component or `Entity`; `Vec2` is a struct | `count_non_component.12am` | 9 | Queries |
| the tag `Frozen` can't be a term of `World.count`; hint: `World.count(Entity, with: Frozen)` | `count_tag_as_term.12am` | 6 | Queries |
| the default of the field `face` is not a constant expression: `rand` can't be called in one | `default_not_constant.12am` | 5 | Constants; Structs: Defaults |
| `delete_at` needs a list that can be modified; `batch` is the read-only parameter of `on` | `delete_at_readonly_on_param.12am` | 10 | What can be modified; Lists |
| `push` does not take a `do` block; `do … end` is accepted only by the loop forms (`times`, `upto`, `downto`, `each`, `each_with_index`, `loop`, a query `each`, `on`) | `do_block_not_loop.12am` | 5 | Control flow: blocks are not values |
| a function call can't take a `do` block (blocks are not values; `do … end` is accepted only by the loop forms) | `do_block_on_function.12am` | 8 | Control flow: blocks are not values |
| duplicate definition: the field `x` of `Vec2` is already defined on line 5 | `duplicate_field.12am` | 7 | Programs and files (one namespace); Functions |
| duplicate definition: the function `area` is already defined on line 4 (there is no overloading) | `duplicate_function.12am` | 8 | Programs and files (one namespace); Functions |
| duplicate definition: the method `doubled` of `Vec2` is already defined on line 7 | `duplicate_method.12am` | 11 | Programs and files (one namespace); Functions |
| duplicate definition: the parameter `a` of `add` is named twice | `duplicate_parameter.12am` | 4 | Programs and files (one namespace); Functions |
| duplicate definition: the system `move` has the name of the function defined on line 4 (systems and functions share a namespace) | `duplicate_system_function.12am` | 8 | Programs and files (one namespace); Functions |
| duplicate definition: the system `report` is already defined on line 4 | `duplicate_system.12am` | 8 | Programs and files (one namespace); Functions |
| duplicate definition: the type `Vec2` is already defined on line 4 | `duplicate_type.12am` | 8 | Programs and files (one namespace); Functions |
| duplicate definition: the type `Health` is already defined (as a component) on line 4 | `duplicate_type_kinds.12am` | 8 | Programs and files (one namespace); Functions |
| duplicate definition: the variant `Red` of `Team` is already defined on line 5 | `duplicate_variant.12am` | 7 | Programs and files (one namespace); Functions |
| `emit` takes an event; `Vec2` is a struct | `emit_non_event.12am` | 9 | Events |
| 25 independent errors: the first 20 are reported in source order, then a line saying that the rest were not shown | `error_cap.12am` | 4 to 23 | Errors |
| the element type of `[]` can't be inferred: nothing here supplies a type (annotate the variable, e.g. `xs: [Int] = []`) | `empty_list_no_type.12am` | 4 | Lists |
| the element type of `[]` can't be inferred: nothing here supplies a type | `empty_list_no_type_method.12am` | 4 | Lists |
| `add` is not allowed in a GPU system | `gpu_add.12am` | 12 | GPU systems |
| `alive?` is not allowed in a GPU system | `gpu_alive.12am` | 14 | GPU systems |
| `break` can't leave the `each` of a GPU system; the `break` inside the `while` is fine | `gpu_break.12am` | 15 | GPU systems |
| `break` can't leave the `each` of a GPU system, in the modifier form too | `gpu_break_modifier.12am` | 10 | GPU systems |
| can't call `Label.width` in a GPU system, because its receiver is a `Label`, whose field `text` is a `String` | `gpu_call_method_receiver.12am` | 21 | GPU systems |
| can't call `describe` in a GPU system, because it returns a `String`; the body of `describe` is not reported | `gpu_call_returns_string.12am` | 14 | GPU systems |
| can't call `total` in a GPU system, because its parameter `values` is a `[Float]`; the list argument is part of the same report | `gpu_call_takes_list.12am` | 18 | GPU systems |
| `clock` is not allowed in a GPU system | `gpu_clock.12am` | 10 | GPU systems |
| constant `BIG` is 5000000000, which does not fit in 32 bits, in a field default that a constructor in a GPU system uses | `gpu_constant_default_too_big.12am` | 11 | GPU systems |
| constant `STEPS` can't be read in a GPU system, because it is a `[Float]`: a list constant can't be iterated there | `gpu_constant_list.12am` | 12 | GPU systems |
| constant `TITLE` can't be read in a GPU system, because it is a `String` | `gpu_constant_string.12am` | 12 | GPU systems |
| constant `SPAN` is 4294967296, which does not fit in 32 bits; `QUARTER`, made from it, fits and is fine on line 14 | `gpu_constant_too_big.12am` | 15 | GPU systems |
| constant `BELOW` is -2147483649, which does not fit in 32 bits; `LOWEST`, -2147483648, is fine on line 13 | `gpu_constant_too_small.12am` | 14 | GPU systems |
| `despawn` is not allowed in a GPU system | `gpu_despawn.12am` | 10 | GPU systems |
| nested `each` inside a branch in a GPU system | `gpu_each_in_branch.12am` | 11 | GPU systems |
| nested `each` inside a loop in a GPU system | `gpu_each_in_loop.12am` | 11 | GPU systems |
| second nested `each` in a GPU block | `gpu_each_second.12am` | 18 | GPU systems |
| `each` nested two deep in a GPU system | `gpu_each_two_deep.12am` | 11 | GPU systems |
| `emit` is not allowed in a GPU system | `gpu_emit.12am` | 14 | GPU systems |
| `exit` is not allowed in a GPU system | `gpu_exit.12am` | 10 | GPU systems |
| can't call `name_of` in a function called from a GPU system, because it returns a `String` | `gpu_function_bad_call.12am` | 13 | GPU systems |
| constant `BIG` is 5000000000, which does not fit in 32 bits, in a function called from a GPU system; the ordinary call on line 20 is fine | `gpu_function_constant_too_big.12am` | 11 | GPU systems |
| `each` is not allowed in a function called from a GPU system, with a note naming the system and the call; the write through its parameter is not a second error | `gpu_function_each.12am` | 10 | GPU systems |
| 4294967296 does not fit in 32 bits, in a function called from a GPU system | `gpu_function_int_literal.12am` | 9 | GPU systems |
| `String` values are not allowed in a function called from a GPU system | `gpu_function_local_string.12am` | 9 | GPU systems |
| `puts` is not allowed in a function called from a GPU system, once, though two systems and three calls reach it | `gpu_function_puts.12am` | 9 | GPU systems |
| resource field `Config.mask` can't be read in a function called from a GPU system, because it is a `UInt32`; the ordinary call on line 22 is fine | `gpu_function_resource_uint32.12am` | 13 | GPU systems |
| `despawn` is not allowed in a function called from a GPU system, reached through another function and a method | `gpu_function_transitive.12am` | 18 | GPU systems |
| `Cell` can't be built in a function called from a GPU system, because its field `mask` is a `UInt32`; the ordinary call on line 24 is fine | `gpu_function_uint32_construct.12am` | 14 | GPU systems |
| field `mask` of `Cell` can't be read in a function called from a GPU system, for a method that reads its receiver's `UInt32` field by name; the method `older` on line 27 is fine, and so is the ordinary call on line 32 | `gpu_function_uint32_read_field.12am` | 13 | GPU systems |
| `World.count` is not allowed in a function called from a GPU system | `gpu_function_world_count.12am` | 9 | GPU systems |
| can't assign to `Stats.calls` in a function called from a GPU system | `gpu_function_write_resource.12am` | 13 | GPU systems |
| `has?` is not allowed in a GPU system | `gpu_has.12am` | 12 | GPU systems |
| `entity[Body]` is not allowed in a GPU system | `gpu_index_component.12am` | 14 | GPU systems |
| `entity[Body]` is not allowed in a GPU system, when it is assigned through too | `gpu_index_component_write.12am` | 14 | GPU systems |
| 4294967296 does not fit in 32 bits, for a field default that a constructor in a GPU system uses | `gpu_int_literal_default.12am` | 9 | GPU systems |
| 2147483648 does not fit in 32 bits; 2147483647 is fine | `gpu_int_literal_too_big.12am` | 11 | GPU systems |
| -2147483649 does not fit in 32 bits; -2147483648 is fine | `gpu_int_literal_too_small.12am` | 11 | GPU systems |
| `[Float]` values are not allowed in a GPU system: a list can't be iterated there | `gpu_list_iteration.12am` | 10 | GPU systems |
| `[Int]` values are not allowed in a GPU system; the later uses of `steps` are quiet | `gpu_list_literal.12am` | 10 | GPU systems |
| GPU system `idle` has no `each` | `gpu_no_each.12am` | 4 | GPU systems |
| `on` is not allowed in a GPU system | `gpu_on.12am` | 14 | GPU systems |
| `print` is not allowed in a GPU system; the interpolated string is part of the same report | `gpu_output_print.12am` | 10 | GPU systems |
| `puts` is not allowed in a GPU system | `gpu_output_puts.12am` | 10 | GPU systems |
| call to recursive function `factorial` in a GPU system: `factorial` calls itself | `gpu_recursion_direct.12am` | 15 | GPU systems |
| call to recursive function `even?` in a GPU system: `even?` calls `odd?`, which calls `even?` | `gpu_recursion_mutual.12am` | 21 | GPU systems |
| call to recursive function `Vec2.+` in a GPU system, when `+=` calls the operator: `Vec2.+` calls `Vec2.doubled`, which calls `Vec2.+` | `gpu_recursion_operator.12am` | 25 | GPU systems |
| call to `spread` in a GPU system reaches recursive function `fib`: `spread` calls `fib`, which calls itself | `gpu_recursion_reached.12am` | 19 | GPU systems |
| `remove` is not allowed in a GPU system | `gpu_remove.12am` | 12 | GPU systems |
| resource field `Config.steps` can't be read in a GPU system, because it is a `[Float]`, also when only an element is read | `gpu_resource_list.12am` | 14 | GPU systems |
| resource field `Config.title` can't be read in a GPU system, because it is a `String`; `Config.scale` is fine | `gpu_resource_string.12am` | 16 | GPU systems |
| resource field `Config.path` can't be read in a GPU system, because it is a `Path`, whose field `points` is a `[Float]` | `gpu_resource_struct_with_list.12am` | 19 | GPU systems |
| resource field `Config.mask` can't be read in a GPU system, because it is a `UInt32`; hint to declare it `Int32`; `Config.scale` is fine | `gpu_resource_uint32.12am` | 17 | GPU systems |
| `return` is not allowed in a GPU system | `gpu_return.12am` | 10 | GPU systems |
| `return` is not allowed in a GPU system, inside the nested `each` and inside a loop too | `gpu_return_in_nested.12am` | 11, 14 | GPU systems |
| four independent errors in one GPU block, in source order: `entity[Health]`, `puts`, the assignment to `Stats.total`, and `return` | `gpu_several.12am` | 18, 19, 20, 21 | GPU systems |
| `sleep` is not allowed in a GPU system | `gpu_sleep.12am` | 10 | GPU systems |
| `spawn` is not allowed in a GPU system | `gpu_spawn.12am` | 11 | GPU systems |
| `srand` is not allowed in a GPU system, with a seed and without | `gpu_srand.12am` | 10, 11 | GPU systems |
| statement outside `each` in a GPU system, once for each statement | `gpu_statement_outside_each.12am` | 9, 13 | GPU systems |
| `String` values are not allowed in a GPU system, once for each operand of `==` that makes one | `gpu_string_compare.12am` | 10 | GPU systems |
| `String` values are not allowed in a GPU system, once for the whole interpolated string | `gpu_string_interpolation.12am` | 10 | GPU systems |
| `String` values are not allowed in a GPU system; the later use of `name` is quiet | `gpu_string_literal.12am` | 10 | GPU systems |
| `Route` values are not allowed in a GPU system, because the field `path.points` is a `[Float]` | `gpu_struct_with_list_nested.12am` | 19 | GPU systems |
| `Label` values are not allowed in a GPU system, because the field `text` is a `String` | `gpu_struct_with_string.12am` | 15 | GPU systems |
| only the term `Position` is reported; `Named` and `Trail`, which hold a string and a list, are fine in `with:` and `without:` | `gpu_term_filter_unrestricted.12am` | 17 | GPU systems |
| `Position` can't be a term of an `each` in a GPU system, because its fields `x` and `y` are 64-bit `Float`s; hint to declare them `Float32` | `gpu_term_float_field.12am` | 11 | GPU systems |
| `Health` can't be a term of an `each` in a GPU system, because its field `hp` is a 64-bit `Int`; hint to declare it `Int32` | `gpu_term_int_field.12am` | 10 | GPU systems |
| `Trail` can't be a term of an `each` in a GPU system, because its field `points` is a `[Float32]` | `gpu_term_list_field.12am` | 10 | GPU systems |
| `Mass` can't be a term of an `each` in a GPU system, as a term of the nested `each` and when it is only read | `gpu_term_nested_each.12am` | 14 | GPU systems |
| `Named` can't be a term of an `each` in a GPU system, because its field `name` is a `String` | `gpu_term_string_field.12am` | 10 | GPU systems |
| `Motion` can't be a term of an `each` in a GPU system, because its fields `velocity.x` and `velocity.y`, inside a struct, are 64-bit `Float`s | `gpu_term_struct_field.12am` | 15 | GPU systems |
| `to_s` is not allowed in a GPU system | `gpu_to_s.12am` | 10 | GPU systems |
| can't assign to `cell.mask` in a GPU system, because it is a `UInt32`; the copy on line 19 and the assignment to `cell.age` on line 20 are fine | `gpu_uint32_assign.12am` | 21 | GPU systems |
| can't assign to `cell.stamp.serial` in a GPU system, for a `UInt32` field inside a struct field of a query parameter; the copy of the whole struct on line 16 is fine | `gpu_uint32_assign_term_field.12am` | 17 | GPU systems |
| can't assign to `grid.cell.mask` in a GPU system, once, for `+=` on a `UInt32` field inside a struct | `gpu_uint32_compound_assign.12am` | 25 | GPU systems |
| `Cell` can't be built in a GPU system, because its field `mask` is a `UInt32`; hint to declare it `Int32` | `gpu_uint32_construct.12am` | 15 | GPU systems |
| `Cell` can't be built in a GPU system, for a field default that a constructor there uses; `Grid` itself has no `UInt32` field, and `Grid(body.n, Config.cell)` on line 24 is fine | `gpu_uint32_construct_default.12am` | 15 | GPU systems |
| field `mask` of `Cell` can't be read in a GPU system, on the result of a call; the call itself, which takes and returns a `Cell`, is fine, and so is `.age` on line 24 | `gpu_uint32_read_call_result.12am` | 25 | GPU systems |
| field `mask` of `Cell` can't be read in a GPU system, inside a struct constant; the reads of `START.width` and `START.cell.age` on line 22 are fine | `gpu_uint32_read_constant_field.12am` | 23 | GPU systems |
| field `mask` of `Cell` can't be read in a GPU system, because it is a `UInt32`; the read of the other field `age` on line 19 is fine | `gpu_uint32_read_field.12am` | 20 | GPU systems |
| field `mask` of `Cell` can't be read in a GPU system, through a query parameter; `Cell` itself is fine as a term, and so is the assignment to `cell.age` on line 11 | `gpu_uint32_read_term_field.12am` | 12 | GPU systems |
| `World.stop`, `World.tick` and `World.run` are not allowed in a GPU system | `gpu_world_control.12am` | 10, 11, 12 | GPU systems |
| `World.count` is not allowed in a GPU system, with a query and without | `gpu_world_count.12am` | 10, 11 | GPU systems |
| can't assign to `e` and to `other` in a GPU system: `Entity` parameters are read-only there | `gpu_write_entity_param.12am` | 10, 12 | GPU systems |
| can't assign to `b.x` in a GPU system: `b` is a parameter of the nested `each` | `gpu_write_nested_param.12am` | 11 | GPU systems |
| can't assign to `b` in a GPU system: `b` is a parameter of the nested `each` | `gpu_write_nested_param_whole.12am` | 11 | GPU systems |
| can't assign to `Stats.total` in a GPU system: resources are read-only | `gpu_write_resource.12am` | 14 | GPU systems |
| can't assign to `Wind.force.x` in a GPU system: resources are read-only, their parts too | `gpu_write_resource_field.12am` | 19 | GPU systems |
| can't assign to `Time.dt` in a GPU system: `Time` is read-only | `gpu_write_time_dt.12am` | 10 | GPU systems |
| `has?` takes a component or tag; `Vec2` is a struct | `has_non_component.12am` | 10 | Entities |
| can't assign to an element of `xs`: a function parameter is read-only | `index_assign_readonly_param.12am` | 5 | What can be modified; Lists |
| type mismatch: `e[Health]` is a `Health`, found an `Armor` | `index_component_wrong_value.12am` | 13 | Entities |
| an entity is indexed by a component type; `Vec2` is a struct | `index_non_component.12am` | 10 | Entities |
| type mismatch: the argument `xs` of `total` is a `[Float]`, found a `[Float32]` (the two list types do not convert to each other) | `list_float32_argument.12am` | 17 | Compact field types |
| type mismatch: `plain` is declared `[Float]`, found a `[Float32]` (the two list types do not convert to each other) | `list_float32_to_float.12am` | 9 | Compact field types |
| type mismatch: the field `heights` of `Mesh` is a `[Float32]`, found a `[Float]` (the two list types do not convert to each other) | `list_float_to_float32.12am` | 10 | Compact field types |
| type mismatch: `plain` is declared `[Int]`, found a `[UInt8]` (lists with different element storage are different types) | `list_uint8_to_int.12am` | 5 | Compact field types |
| `next` outside a loop | `next_outside_loop.12am` | 5 | Control flow |
| `on` takes an event type; `Position` is a component | `on_non_event.12am` | 9 | Events |
| `pop` needs a list that can be modified; `xs` is a function parameter and is read-only | `pop_readonly_list.12am` | 5 | What can be modified; Lists |
| `push` needs a list that is a place (a variable, a field, an element); the result of `numbers()` is not one | `push_not_a_place.12am` | 8 | What can be modified; Lists |
| `push` needs a list that can be modified; `PRIMES` is a constant | `push_readonly_constant.12am` | 6 | What can be modified; Lists |
| `push` needs a list that can be modified; `row` is the read-only parameter of a list iteration | `push_readonly_iteration_param.12am` | 6 | What can be modified; Lists |
| `push` needs a list that can be modified; `xs` is a function parameter and is read-only | `push_readonly_list.12am` | 5 | What can be modified; Lists |
| `with:` takes components and tags; `Team` is an enum | `query_filter_non_component.12am` | 14 | Queries |
| a query term must be a component or `Entity`; `Vec2` is a struct | `query_non_component.12am` | 9 | Queries |
| syntax error: a query needs at least one term before its filters (use `Entity` to visit every entity) | `query_no_terms.12am` | 11 | Queries |
| the query has 2 terms but the block takes 1 parameter | `query_param_count.12am` | 14 | Queries |
| the query has 1 term but the block takes no parameters | `query_param_count_none.12am` | 9 | Queries |
| the query has 1 term but the block takes 2 parameters | `query_param_count_too_many.12am` | 9 | Queries |
| the type `Position` appears twice in the query (as a term and in `with:`) | `query_repeated_in_filter.12am` | 9 | Queries |
| the type `Position` appears twice in the query | `query_repeated_type.12am` | 9 | Queries |
| a query term must be a component or `Entity`; `Score` is a resource | `query_resource_term.12am` | 8 | Queries |
| the tag `Frozen` can't be a query term (a tag has no value to bind); hint: filter with `with: Frozen` | `query_tag_as_term.12am` | 11 | Queries |
| unknown query filter `withou`; the label names the two filters there are, `with:` and `without:` | `query_unknown_filter.12am` | 11 | Queries |
| the type `Frozen` appears twice in the query (in `with:` and in `without:`) | `query_with_and_without_same.12am` | 11 | Queries |
| can't assign to an element of `PRIMES`: a constant can't be changed | `readonly_constant_element.12am` | 6 | What can be modified; Lists |
| can't assign to `ORIGIN.x`: `ORIGIN` is a constant, and a part of a constant can't be changed | `readonly_constant_field.12am` | 11 | What can be modified; Lists |
| can't assign to `LIMIT`: a constant can't be changed | `readonly_constant.12am` | 6 | What can be modified; Lists |
| can't assign to `LIMIT`: a constant can't be changed | `readonly_constant_in_function.12am` | 7 | What can be modified; Lists |
| can't assign to `i`: the parameter of a counting loop is read-only | `readonly_counting_param.12am` | 5 | What can be modified; Lists |
| can't assign to `i`: the index parameter of `each_with_index` is read-only | `readonly_each_with_index_param.12am` | 6 | What can be modified; Lists |
| can't assign to `v.y`: `v` is a function parameter, and a part of a read-only value is read-only | `readonly_function_param_field.12am` | 10 | What can be modified; Lists |
| can't assign to `n`: a function parameter is read-only | `readonly_function_param.12am` | 5 | What can be modified; Lists |
| can't assign to `pt.x`: `pt` is the parameter of a list iteration, and a part of a read-only value is read-only | `readonly_list_iteration_param_field.12am` | 11 | What can be modified; Lists |
| can't assign to `x`: the parameter of a list iteration is read-only (to update elements, loop over indexes) | `readonly_list_iteration_param.12am` | 6 | What can be modified; Lists |
| can't assign to `k`: a method parameter is read-only | `readonly_method_param.12am` | 9 | What can be modified; Lists |
| can't assign to `hit.damage`: the parameter of `on` is a read-only copy | `readonly_on_param.12am` | 10 | What can be modified; Lists |
| can't assign to `hit`: the parameter of `on` is read-only | `readonly_on_param_whole.12am` | 10 | What can be modified; Lists |
| can't assign to `self.count`: `self` is read-only (methods cannot modify the receiver) | `readonly_self_dot_field.12am` | 8 | What can be modified; Lists |
| can't assign to the field `count` inside a method: methods cannot modify the receiver | `readonly_self_field.12am` | 8 | What can be modified; Lists |
| can't assign to `Time.elapsed`: it is read-only | `readonly_time_elapsed.12am` | 4 | What can be modified; Lists |
| can't assign to `Time.tick`: it is read-only | `readonly_time_tick.12am` | 4 | What can be modified; Lists |
| can't assign to `i`: the parameter of a counting loop is read-only | `readonly_upto_param.12am` | 5 | What can be modified; Lists |
| the recursive function `fact` needs a declared return type: its call to itself needs a type that is still being inferred | `recursion_direct_no_type.12am` | 6 | Functions |
| the recursive function `is_even` needs a declared return type: neither `is_even` nor `is_odd` declares one | `recursion_mutual_no_type.12am` | 11 | Functions |
| `remove` takes components and tags; `Vec2` is a struct | `remove_non_component.12am` | 10 | Entities |
| can't find the required file `no_such_helper_file.12am` (looked for next to this file) | `require_not_found.12am` | 4 | Programs and files |
| `require` is allowed at the top level only | `require_not_top_level.12am` | 7 | Programs and files |
| the resource `Score` is not a value type and can't be the type of a field | `resource_as_field_type.12am` | 9 | Resources |
| the resource `Score` as a whole is not a value; read its fields (`Score.points`) | `resource_as_value.12am` | 8 | Resources |
| the field `points` of the resource `Score` needs a default | `resource_field_no_default.12am` | 5 | Resources |
| unknown variable `square`: it was created inside the `do` block (line 5) and does not exist after it | `scope_after_do_block.12am` | 7 | Variables and values; Programs and files |
| unknown variable `label`: it was created inside the `if` (line 6) and does not exist after it; create it before the `if` | `scope_after_if.12am` | 8 | Variables and values; Programs and files |
| unknown variable `seen`: it was created inside the `while` body (line 6) and does not exist after the loop | `scope_after_while.12am` | 9 | Variables and values; Programs and files |
| unknown variable `i`: a block parameter does not exist after its block | `scope_block_param_after_block.12am` | 7 | Variables and values; Programs and files |
| `self` can be used only inside a method of a struct or component | `self_outside_method.12am` | 5 | Structs: Methods |
| the component `Position` can't be listed bare in `spawn`: its fields `x` and `y` have no defaults | `spawn_bare_no_defaults.12am` | 9 | Entities |
| the component `Health` can't be listed bare in `spawn`: its field `hp` has no default | `spawn_bare_partial_defaults.12am` | 9 | Entities |
| `spawn` takes components and tags; `Vec2` is a struct | `spawn_non_component.12am` | 9 | Entities |
| `spawn` lists the tag `Player` twice | `spawn_tag_twice.12am` | 6 | Entities |
| `spawn` lists the component `Position` twice | `spawn_type_twice.12am` | 9 | Entities |
| the struct `Node` contains itself through the field `child` (allowed only through a list, `[Node]`) | `struct_contains_itself.12am` | 6 | Structs |
| the struct `Parent` contains itself through `Parent.child` and `Child.parent`; one diagnostic for the cycle | `struct_contains_itself_mutual.12am` | 5 | Structs |
| `==` is provided for every struct and can't be redefined (it is also not one of the operator names a method may have) | `struct_redefines_equality.12am` | 8 | Structs |
| `to_s` is provided for every struct and can't be redefined | `struct_redefines_to_s.12am` | 8 | Structs |
| syntax error: unexpected `=` inside an expression (assignment is a statement, not an expression) | `syntax_assignment_as_expression.12am` | 5 | Grammar; Lexical structure |
| syntax error: unexpected `4` after `double` (a call with arguments needs parentheses) | `syntax_call_without_parentheses.12am` | 8 | Grammar; Lexical structure |
| syntax error: `gpu` is a keyword (it goes before `system`) and can't name a variable | `syntax_gpu_reserved.12am` | 4 | Grammar; Lexical structure |
| syntax error: `tag` is a keyword and can't name a variable; a type name is expected after `tag`, found `=` | `syntax_keyword_as_variable.12am` | 4 | Grammar; Lexical structure |
| syntax error: a type name (beginning with an uppercase letter) is expected after `struct`, found `vec2` | `syntax_lowercase_type_name.12am` | 4 | Grammar; Lexical structure |
| syntax error: the parameter `n` needs a type (`n: Type`); found `)` where `:` is expected | `syntax_param_without_type.12am` | 4 | Grammar; Lexical structure |
| syntax error: unexpected `def` in a resource body (a resource holds fields only) | `syntax_resource_method.12am` | 7 | Grammar; Lexical structure |
| syntax error: unexpected `end`; no block is open | `syntax_stray_end.12am` | 6 | Grammar; Lexical structure |
| syntax error: unexpected `(` after the system name (a system has no parameters) | `syntax_system_with_parameters.12am` | 4 | Grammar; Lexical structure |
| the `def` block is never closed (an `end` is missing); the diagnostic points at the `def` that opened it, not at the end of the file | `syntax_unclosed_def.12am` | 4 | Grammar; Lexical structure |
| the `do` block is never closed (an `end` is missing); the diagnostic points at the `do` that opened it, not at the end of the file | `syntax_unclosed_do.12am` | 4 | Grammar; Lexical structure |
| the `if` block is never closed (an `end` is missing); the diagnostic points at the `if` that opened it, not at the end of the file | `syntax_unclosed_if.12am` | 5 | Grammar; Lexical structure |
| the `struct` block is never closed (an `end` is missing); the diagnostic points at the `struct` that opened it, not at the end of the file | `syntax_unclosed_struct.12am` | 4 | Grammar; Lexical structure |
| the `system` block is never closed (an `end` is missing); the diagnostic points at the `system` that opened it, not at the end of the file | `syntax_unclosed_system.12am` | 4 | Grammar; Lexical structure |
| the `while` block is never closed (an `end` is missing); the diagnostic points at the `while` that opened it, not at the end of the file | `syntax_unclosed_while.12am` | 5 | Grammar; Lexical structure |
| syntax error: unexpected `)` where an expression is expected | `syntax_unexpected_token.12am` | 5 | Grammar; Lexical structure |
| syntax error: unexpected `elsif` (an `unless` takes only an optional `else`) | `syntax_unless_elsif.12am` | 8 | Grammar; Lexical structure |
| unterminated string literal; the diagnostic points at the opening quote | `syntax_unterminated_string.12am` | 4 | Grammar; Lexical structure |
| a variable name can't end in `?` (only function and method names may) | `syntax_variable_question_mark.12am` | 4 | Grammar; Lexical structure |
| `move` is a system; systems are not called by name (they run on `World.tick`) | `system_called.12am` | 8 | Systems and ticks |
| `move` is a system; systems are not called by name (they run on `World.tick`) | `system_called_with_parens.12am` | 8 | Systems and ticks |
| the tag `Frozen` is not a value type and can't be the type of a field | `tag_as_field_type.12am` | 7 | Components and tags |
| the tag `Frozen` has no value and can't be constructed; write the bare name `Frozen` | `tag_constructed.12am` | 6 | Components and tags |
| the tag `Frozen` has no value and can't be read from an entity; use `e.has?(Frozen)` | `tag_indexed.12am` | 7 | Components and tags |
| the tag `Frozen` has no value and can't be stored in a variable | `tag_in_variable.12am` | 6 | Components and tags |
| three independent errors, ALL reported in one run, in source order: | `three_errors.12am` | 9, 13, 17 | Errors |
| unknown variable `total` inside the function `show`: top-level variables are not visible in functions (state belongs in a resource or on an entity) | `toplevel_variable_in_function.12am` | 7 | Variables and values; Programs and files |
| unknown variable `speed` inside the system `move`: top-level variables are not visible in systems (state belongs in a resource or on an entity) | `toplevel_variable_in_system.12am` | 7 | Variables and values; Programs and files |
| type mismatch: `count` is declared `Int`, found a `Float` (a `Float` does not convert to `Int`) | `type_annotation.12am` | 4 | Types; Expressions |
| type mismatch: the field `y` of `Vec2` is a `Float`, found a `String` | `type_argument_constructor.12am` | 9 | Types; Expressions |
| type mismatch: the argument `n` of `double` is an `Int`, found a `Float` (a `Float` does not convert to `Int`; use `to_i`, `floor`, `ceil` or `round`) | `type_argument_float_to_int.12am` | 8 | Types; Expressions |
| type mismatch: the argument `n` of `double` is an `Int`, found a `String` | `type_argument.12am` | 8 | Types; Expressions |
| type mismatch: a `case` used as a value needs branches of one type; found a `String` and an `Int` | `type_case_branches.12am` | 7 | Types; Expressions |
| type mismatch: the `case` value is an `Int`, but this `when` candidate is a `String` | `type_case_when.12am` | 7 | Types; Expressions |
| type mismatch: `==` can't compare an `Int` with a `String` | `type_equality_different_types.12am` | 5 | Types; Expressions |
| type mismatch: the field `hp` of `Health` is an `Int`, found a `String` | `type_field_assign_component.12am` | 9 | Types; Expressions |
| type mismatch: the field `x` of `Vec2` is a `Float`, found a `String` | `type_field_assign.12am` | 10 | Types; Expressions |
| type mismatch: the default of the field `hp` must be an `Int`, found a `String` | `type_field_default.12am` | 5 | Types; Expressions |
| type mismatch: an `if` used as a value needs branches of one type; found a `String` and an `Int` | `type_if_branches.12am` | 5 | Types; Expressions |
| type mismatch: a list index must be an `Int`, found a `String` | `type_index_not_int.12am` | 5 | Types; Expressions |
| type mismatch: an element of a `[Int]` is an `Int`, found a `String` | `type_list_element_assign.12am` | 5 | Types; Expressions |
| type mismatch: the elements of a list literal share one type; the first is an `Int`, this one is a `String` | `type_list_elements.12am` | 4 | Types; Expressions |
| type mismatch: `push` on a `[Int]` takes an `Int`, found a `String` | `type_list_push.12am` | 5 | Types; Expressions |
| type mismatch: `+` can't be applied to an `Int` and a `String` | `type_operands.12am` | 4 | Types; Expressions |
| type mismatch: `+` can't join a `[Int]` and a `[String]` | `type_operands_list_concat.12am` | 6 | Types; Expressions |
| type mismatch: the operands of `&&` must be `Bool`, found an `Int` | `type_operands_logical.12am` | 6 | Types; Expressions |
| type mismatch: unary `-` can't be applied to a `String` | `type_operands_negate_string.12am` | 5 | Types; Expressions |
| type mismatch: `!` needs a `Bool`, found an `Int` | `type_operands_not.12am` | 5 | Types; Expressions |
| type mismatch: the right operand of `+` on `Vec2` (parameter `other`) is a `Vec2`, found an `Int` | `type_operands_operator_method_argument.12am` | 14 | Types; Expressions |
| type mismatch: `*` on a `String` needs an `Int` count, found a `String` | `type_operands_repeat_count.12am` | 4 | Types; Expressions |
| `+` is not defined for `Vec2` (the struct has no method named `+`) | `type_operands_struct_no_operator.12am` | 11 | Types; Expressions |
| type mismatch: `*` can't be applied to an `Int` and a `Vec2` (an operator method applies only when the struct is the left operand) | `type_operands_struct_on_right.12am` | 14 | Types; Expressions |
| type mismatch: `Score.points` is an `Int`, found a `Float` (a `Float` does not convert to `Int`) | `type_resource_field_assign.12am` | 8 | Types; Expressions |
| type mismatch: `name_of` returns `String`, but its last expression is an `Int` | `type_return.12am` | 5 | Types; Expressions |
| the return type of `pick` can't be inferred: it returns a `String` on line 5 and an `Int` on line 6 | `type_return_inconsistent.12am` | 6 | Types; Expressions |
| `answer` declares the return type `Int`, but its body can end without a value | `type_return_missing_value.12am` | 4 | Types; Expressions |
| type mismatch: `label` returns `String`, but `return` is given an `Int` | `type_return_statement.12am` | 5 | Types; Expressions |
| type mismatch: the branches of `? :` are an `Int` and a `String` | `type_ternary_branches.12am` | 5 | Types; Expressions |
| unknown component `Postion`, suggesting `Position` | `unknown_component_query_suggest.12am` | 14 | Errors (suggestions); the section of the thing named |
| unknown constant (or type) `GRAVTY`, suggesting `GRAVITY` | `unknown_constant_suggest.12am` | 6 | Errors (suggestions); the section of the thing named |
| unknown variant `Purple` of the enum `Team`, with no suggestion | `unknown_enum_variant.12am` | 10 | Errors (suggestions); the section of the thing named |
| unknown variant `Gren` of the enum `Team`, suggesting `Green` | `unknown_enum_variant_suggest.12am` | 10 | Errors (suggestions); the section of the thing named |
| unknown field `heigt` on `Rect`, suggesting `height` | `unknown_field_assign_suggest.12am` | 10 | Errors (suggestions); the section of the thing named |
| unknown field (or method) `opacity` on `Rect`, with no suggestion | `unknown_field.12am` | 10 | Errors (suggestions); the section of the thing named |
| unknown field (or method) `depth` on `Position`, with no suggestion | `unknown_field_query_param.12am` | 11 | Errors (suggestions); the section of the thing named |
| unknown field (or method) `widt` on `Rect`, suggesting `width` | `unknown_field_suggest.12am` | 10 | Errors (suggestions); the section of the thing named |
| unknown function `frobnicate`, with no suggestion | `unknown_function.12am` | 8 | Errors (suggestions); the section of the thing named |
| unknown function `total`: `total(…)` is always a function call, and `total` is only a local variable | `unknown_function_local_variable.12am` | 5 | Errors (suggestions); the section of the thing named |
| unknown function `doubl`, suggesting `double` | `unknown_function_suggest.12am` | 8 | Errors (suggestions); the section of the thing named |
| unknown constant `Math::TAU`, with no suggestion (`Math` has `PI` and `E`) | `unknown_math_constant.12am` | 4 | Errors (suggestions); the section of the thing named |
| unknown function `Math.sqr`, suggesting `sqrt` | `unknown_math_function_suggest.12am` | 4 | Errors (suggestions); the section of the thing named |
| unknown method `frobnicate` on a list (`[Int]`), with no suggestion | `unknown_method_builtin.12am` | 5 | Errors (suggestions); the section of the thing named |
| unknown method `explode` on `Rect`, with no suggestion | `unknown_method.12am` | 14 | Errors (suggestions); the section of the thing named |
| unknown method `lengh` on a list (`[Int]`), suggesting `length` | `unknown_method_list_suggest.12am` | 5 | Errors (suggestions); the section of the thing named |
| unknown method `upcas` on `String`, suggesting `upcase` | `unknown_method_string_suggest.12am` | 5 | Errors (suggestions); the section of the thing named |
| unknown method `scale` on `Rect`, suggesting `scaled` | `unknown_method_suggest.12am` | 14 | Errors (suggestions); the section of the thing named |
| unknown method `upcase` on `Int` (it exists on `String` only) | `unknown_method_wrong_type.12am` | 5 | Errors (suggestions); the section of the thing named |
| unknown field `point` on the resource `Score`, suggesting `points` | `unknown_resource_field_suggest.12am` | 9 | Errors (suggestions); the section of the thing named |
| unknown component or tag `Playr` in `with:`, suggesting `Player` | `unknown_tag_filter_suggest.12am` | 11 | Errors (suggestions); the section of the thing named |
| unknown field `elapse` on `Time`, suggesting `elapsed` | `unknown_time_field_suggest.12am` | 4 | Errors (suggestions); the section of the thing named |
| unknown type `Flot`, suggesting the built-in `Float` | `unknown_type_builtin_suggest.12am` | 5 | Errors (suggestions); the section of the thing named |
| unknown type `Vec3`, suggesting `Vec2` | `unknown_type_constructor_suggest.12am` | 9 | Errors (suggestions); the section of the thing named |
| unknown type `Polygon`, with no suggestion | `unknown_type.12am` | 5 | Errors (suggestions); the section of the thing named |
| unknown type `Quaternion`, with no suggestion | `unknown_type_parameter.12am` | 4 | Errors (suggestions); the section of the thing named |
| unknown type `Vec3`, suggesting `Vec2` | `unknown_type_suggest.12am` | 10 | Errors (suggestions); the section of the thing named |
| unknown variable (or function) `zzyzx`, with no suggestion | `unknown_variable.12am` | 6 | Errors (suggestions); the section of the thing named |
| unknown variable (or function) `totl`, suggesting `total` | `unknown_variable_suggest.12am` | 5 | Errors (suggestions); the section of the thing named |
| unknown `World` operation `tik`, suggesting `tick` | `unknown_world_method_suggest.12am` | 8 | Errors (suggestions); the section of the thing named |
| an `if` used as a value needs an `else` branch | `value_if_without_else.12am` | 5 | Types; Expressions |
| `greet` returns nothing, so its call can't be used as a value | `void_as_value.12am` | 8 | Functions |
| `greet` returns nothing, so its call can't be used as a value (here, as an argument of `puts`) | `void_in_expression.12am` | 8 | Functions |
| `World.tick` and `World.run` are not allowed in the body of a query `each`: at the top level, nested, in a function and in a system; `World.stop` there, and a tick in a list's `each`, are fine | `world_tick_in_each.12am` | 14, 16, 23, 29 | Systems and ticks |
