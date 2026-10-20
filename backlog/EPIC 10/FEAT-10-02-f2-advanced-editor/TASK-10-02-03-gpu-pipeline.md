# task: port recipe ops to wgpu gpu pipeline

**Feature:** [FEAT-10-02-f2-advanced-editor](FEAT-10-02-f2-advanced-editor.md) · **Epic:** [EPIC-10 — Future features](../EPIC-10.md) · **Status:** planned (deferred)

## Description
Port recipe ops to `wgpu` shaders for real-time full-resolution editing, with the same shaders on all platforms (PLAN §7 F2 item 3). Depends on the EPIC-04 recipe op set (task 02 grows it).

## Functional specs
- Full-res preview updates in real time while adjusting recipe ops.
- GPU-rendered final output matches the CPU path (pixel-identical or within a documented tolerance).

## Technical specs
- `wgpu` compute/render shaders per recipe op; op-level shader/CPU dual path selected by capability.
- Tolerance documented per op; CPU path remains the golden reference.

## Testing criteria (TDD)
- integration: render the golden-image fixture set through both CPU and GPU paths; assert per-op tolerance compliance in CI.

## Acceptance criteria
- [ ] F2-003: GPU pipeline produces output matching the CPU path (or documented tolerance) across golden fixtures, enabling real-time full-res editing.
