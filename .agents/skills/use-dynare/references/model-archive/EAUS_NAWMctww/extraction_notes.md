# EAUS_NAWMctww Extraction Notes

## Source Match

- `raw/mmb_mineru/model_index.csv` row: `EAUS_NAWMctww`.
- Expected title/authors: "Fiscal consolidation strategy"; Cogan, Taylor, Wieland, Wolters.
- First 80 Markdown lines checked in `raw/mmb_mineru/runs/eaus_nawmctww__fiscal_consolidation_strategy__97cf0e5f/full.md`.
- Observed title/authors match the row. No source-title mismatch recorded.
- `raw_pdf_path` exists at `raw/mmb_papers/Fiscal consolidation strategy.pdf`; PDF body was not read.
- Appendix normalization `docs/mmb_appendix_full_normalizations/EAUS_NAWMctww.md` was not present.

## Formula Quality

- Status: `needs_review`.
- The paper provides the central fiscal authority equation, tax/debt feedback rule, household utility and budget constraints, capital accumulation, production and marginal-cost formulas, final-good CES technologies, and monetary policy rule.
- OCR quality for the central equations is usable, but the entry remains a first-pass derivation rather than a formula-by-formula verified transcription.

## Implementation Cross-Check

- `.agents/skills/use-dynare/references/examples/EAUS_NAWMctww_rep.mod` exists and was read only as `implementation_cross_check`.
- Cross-check findings:
  - The replication file is a nonlinear `model;` block, not `model(linear)`.
  - It contains both euro-area (`EA_`) and U.S. (`US_`) blocks, with U.S. variables used as common comparison variables for the fiscal-consolidation application.
  - It includes fiscal shocks/processes for purchases, transfers, consumption taxes, dividend taxes, capital taxes, labor taxes, employee and employer social security taxes, monetary shocks, and productivity shocks.
  - It sets several fiscal persistence parameters to zero to implement specified fiscal-consolidation paths.
- No Dynare execution was performed.

## Deferred Issues

- Full NAWM steady-state derivation remains deferred. The derivation records paper restrictions and implementation calibration values but does not claim a reviewed steady-state solution.
- Calvo wage and price recursions need source-level verification against the original NAWM paper or a reviewed technical appendix before status can advance beyond `needs_review`.
- Exact timing of the foreign-bond accumulation identity should be checked in a targeted source review.
- The archive entry should not be promoted to `.agents/skills/use-dynare/references/model-archive/` until a runnable implementation and validation evidence exist.

