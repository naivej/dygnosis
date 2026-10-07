# Local model libraries: catalog lookup

Read this in SKILL.md "New model", step "Sources", before you write any equation of a modeling or
replication task.

The skill ships two libraries, each with its own catalog. They answer different questions. Search them
separately; usually you need both. The model archive (end of this file) is a third, read-only reference.

---

## Which library answers which question

| Question | Library | Catalog | Folder |
| -------- | ------- | ------- | ------ |
| **How is the economics set up?** FOCs, mechanism design, calibration, timing convention | **Model reference library** (MMB) | `references/catalog.csv` | `references/examples/` |
| **How is the Dynare block written?** Command options, block format, how a feature is used | **Programming library** (Pfeifer DSGE_mod + Dynare course) | `references/catalog-code.csv` | `references/examples-code/` |

> When both have hits, take the block format and syntax from the programming library first, then align
> the economic structure with the model reference library. Do not mix them up, and do not start writing
> after searching only one.

---

## 1. Model reference library (`catalog.csv` + `examples/`)

### What it is

- `references/catalog.csv`: an index of 149 replicable macroeconomic models from the **rep-mmb
  replication archive** of the Macroeconomic Model Database (MMB) (macromodelbase.com/rep-mmb; upstream
  repository IMFS-MMB/mmb-rep).
- `references/examples/<ModelID>.mod`: one Dynare replication `.mod` per paper. **The file name is the
  `ModelID`** (for example `references/examples/EA_GNSS10_rep.mod`).
- These `_rep` files reproduce the original paper without MMB's common policy interface: clean,
  paper-faithful references.

### Columns of `catalog.csv`

`ModelID, Paper, Authors, Year, Journal, ModelType, Economy, Category, KeyFeatures`

- **ModelID** = the file name in `examples/` without `.mod`.
- **Year** is text and can carry notes (for example `2008 (Working Paper; published 2011 …)`); grep it,
  do not compare it as a number.
- **ModelType** = model family and **whether it is linearized** (important; see "Copy warnings").
- **Category** = one of 14 topic buckets (coarse filter, below).
- **KeyFeatures** = free-text mechanism tags (`financial accelerator`, `search-and-matching`,
  `two-country`, `housing`, `Bayesian estimation`, …): the main grep target.

### The 14 categories (filter by bucket first, then grep the mechanism)

| Category | Models |
|----------|--------|
| 1. Baseline NK / Monetary Policy Rules | 14 |
| 2. Estimated DSGE Benchmarks (Smets-Wouters Type) | 17 |
| 3. Financial Accelerator / BGG-type Credit Frictions | 22 |
| 4. Banking Sector / Bank Capital Channel | 18 |
| 5. Labour Market Frictions (Search & Matching) | 10 |
| 6. Open Economy / Multi-Country | 14 |
| 7. Fiscal Policy / Government Spending | 13 |
| 8. Housing / Collateral Constraints | 5 |
| 9. Unconventional Monetary Policy / QE | 6 |
| 10. Energy & Commodities | 4 |
| 11. Money-in-the-Model | 6 |
| 12. Learning & Expectations Formation | 3 |
| 13. Macroprudential Policy | 4 |
| 14. Large Official Policy Models | 13 |

### Lookup steps (model reference library)

1. **Extract the features**: model type, core mechanism, economy.
2. **grep** (149 rows; grep saves context; `rg -i` works the same way):
   - By mechanism: `grep -iE "financial accelerator|BGG" references/catalog.csv`
   - By economy: `grep -i "Euro Area" references/catalog.csv`
   - By category: pick the Category bucket, then filter by KeyFeatures inside it.
3. **Report candidates**: pick the 3–5 closest and tell the user in one line, for example "Close
   models: `<ID>` (paper / mechanism)".
4. **Read the references**: read 1–3 `references/examples/<ModelID>.mod` files and extract variable and
   equation forms, timing convention, calibration, shock setup and steady-state handling. Read the
   relevant blocks only; do not load whole files into context.

---

## 2. Programming library (`catalog-code.csv` + `examples-code/`)

### What it is

- `references/catalog-code.csv`: an index of 89 Dynare programming examples from two sources:
  - **DSGE_mod** (41 rows): Johannes Pfeifer's DSGE_mod repository
    ([github.com/JohannesPfeifer/DSGE_mod](https://github.com/JohannesPfeifer/DSGE_mod)). Each file shows
    how a command or module is used in a paper replication.
  - **Dynare course** (48 rows; `Folder` starts with `Dynare_Course/`): teaching examples from Pfeifer's
    "Advanced Dynare" course, **organized by Dynare feature**, chapter by chapter. The same RBC/NK model
    runs through several chapters, so these show most cleanly how to use a command or option. Prefer them
    when you copy command syntax.

  Both sources show Dynare best practice and the standard way to write commands and modules, not the
  economic structure of a paper.
- `references/examples-code/<Folder>/<CodeID>.mod`: the `.mod` file (plus key `.m`, `.inc` and `.mat`
  helper files). **The folder name is the `Folder` column** (course examples look like
  `Dynare_Course/Chapter_10_forecasting`).

### Course chapters by feature (chapter → reference file)

Each feature reference ends with a course-examples section that lists the course `.mod` paths and what
each one teaches. When you route a request (SKILL.md "Route the task"), read the feature reference; its
last section points to these runnable examples. You can also grep the `DynareFeatures` column of
`catalog-code.csv` directly.

| Course chapter | Feature reference | Course folder `Dynare_Course/` | Representative commands |
| -------------- | ----------------- | ------------------------------ | ----------------------- |
| Ch1 Introduction | stochastic-simulation.md | `Chapter_01_Dynare` | `model(linear)`, `stoch_simul(tex)` |
| Ch2/3 Preprocessor and macro processor | macro-processor.md | (no folder; macro directives appear in Ch9 and Ch11 examples) | `@#include`, `@#if` |
| Ch4 Stochastic simulation | stochastic-simulation.md | `Chapter_04_stoch_simul` | `stoch_simul`, `hp_filter`, `conditional_variance_decomposition` |
| Ch5 Kalman filter / maximum likelihood | estimation.md | `Chapter_05_Kalman_ML` | `calib_smoother` |
| Ch6 Bayesian estimation | estimation.md | `Chapter_06_Bayesian` | `estimation`, `mh_jscale` (acceptance-rate tuning) |
| Ch7 Identification analysis | identification.md | `Chapter_07_Identification` | `identification`, `no_identification_*` |
| Ch8 Higher-order perturbation | higher-order.md | `Chapter_08_Higher_order` | `stoch_simul(order=2/3)`, risk premia |
| Ch9 Method of moments | moments-method.md | `Chapter_09_Method_of_Moments` | `method_of_moments` (`mom_method` = GMM, SMM, IRF_MATCHING) |
| Ch10 Forecasting | forecasting.md | `Chapter_10_forecasting` | `forecast`, `conditional_forecast`, `smoother2histval` |
| Ch11 Perfect foresight | perfect-foresight.md | `Chapter_11_perfect_foresight` | `perfect_foresight_*`, `endval`, `extended_path`, `lmmcp` |
| Ch12 OccBin | occbin.md | `Chapter_12_OccBin` | `occbin_constraints`, `occbin_setup`, `occbin_solver`, `lmmcp` |
| Ch13 Optimal policy | optimal-policy.md | `Chapter_13_optimal_policy` | `ramsey_model`, `discretionary_policy`, `osr` |

### Columns of `catalog-code.csv`

`CodeID, Folder, Paper, Authors, Year, ModelType, Economy, DynareFeatures, Category`

- **CodeID** = the `.mod` file name without `.mod`. A CodeID is not unique across folders
  (`RBC_baseline` is in `RBC_baseline/` and in `Dynare_Course/Chapter_04_stoch_simul/`), so always use
  `Folder` + `CodeID` to build the path.
- **Folder** = the subfolder under `examples-code/`.
- **DynareFeatures** = the Dynare programming features the file shows best: the main grep target.
- **Category** = one of 11 feature buckets (coarse filter):

| Category | Example CodeIDs |
|----------|-----------------|
| 1. RBC / NK Basics | RBC_baseline, Born_Pfeifer_2018_MP |
| 2. NK Linearized | Gali_2015_chapter_3, Gali_2008_chapter_3 |
| 3. NK Nonlinear | Gali_2015_chapter_3_nonlinear |
| 4. TANK / Heterogeneous | Gali_2010, Gali_2010_calib_target |
| 5. Estimation (ML/Bayesian) | Smets_Wouters_2007_45, Ireland_2004, RBC_baseline_first_diff_bayesian |
| 6. Optimal Policy | Gali_2015_chapter_5_commitment, Gali_2015_chapter_5_discretion, Ramsey_Example_* |
| 7. Higher-Order Methods | Basu_Bundick_2017, SGU_2004, Caldara_et_al_2012 |
| 8. Perfect Foresight | Gali_2015_chapter_5_commitment_ZLB, Gali_2015_chapter_5_discretion_ZLB, Solow_* |
| 9. Open Economy | Gali_Monacelli_2005, SGU_2003, Aguiar_Gopinath_2007 |
| 10. Welfare Analysis | RBC_baseline_welfare, Born_Pfeifer_2018_welfare |
| 11. Special Methods | RBC_news_shock_model, NK_linear_forward_guidance, Ascari_Sbordone_2014 |

### Lookup steps (programming library)

1. **Name the programming problem**: find the Dynare feature keyword (for example
   `discretionary_policy`, `lmmcp`, `steadystate.m`, `ramsey_model`, `loglinear`, `news shock`,
   `welfare`).
2. **grep the DynareFeatures column** (89 rows, fast):
   - A command: `grep -iE "ramsey_model|discretionary" references/catalog-code.csv`
   - A block or interface: `grep -i "steadystate.m" references/catalog-code.csv`
   - A feature: `grep -iE "lmmcp|ZLB|zero lower bound" references/catalog-code.csv`
   - A model class: `grep -iE "TANK|hand-to-mouth" references/catalog-code.csv`
3. **Read the reference**: read `references/examples-code/<Folder>/<CodeID>.mod` and extract only the
   relevant blocks (command options, block format, helper-function interface). Do not load whole files.
4. **Need the economic structure too?** Search the model reference library (`catalog.csv`) for the
   mechanism and the timing convention.

---

## Copy warnings

**Model reference library:**
- `ModelType` contains `(linearized)` (21 rows): that `.mod` is a linearized version. Take the equation
  content, mechanism, timing and calibration; do not copy the linearized form. Write the nonlinear model
  (R8), unless the user asks for a linear model or the source gives only the linearized system.
- A reference does not replace Stage 1 (derivation note).

**Programming library:**
- Many examples are written in linearized form (Galí 2008/2015 and others). Take only the syntax pattern;
  the economic structure comes from your derivation, written nonlinear (R8).
- Helper `.m` files (`*_steadystate.m`, `IRF_matching_objective.m`, …) can be adapted and reused.
- Names in the examples do not always follow R5 (for example `beta` and `i` in the Galí files). Rename
  them when you copy into a new model.
- `Dynare_Course/Chapter_11_perfect_foresight/rbcii.mod` uses the obsolete `[mcp='…']` equation tag.
  Write the complementarity condition with `⟂` (ASCII `_|_`) instead (R6; Dygnosis W170).
- `Dynare_Course/Chapter_11_perfect_foresight/nk3_zlb_stoch.mod` uses `max` under `stoch_simul` on
  purpose, to show why perturbation cannot handle the ZLB. Do not copy that pattern (R6; Dygnosis W200).

**Both libraries:** run `dynare_diagnose` on any block you copy (`references/dygnosis-workflow.md`); it
reports deprecated commands and options (W150) and model-block names that your file does not declare
(E020).

---

## How this fits the main flow

- **SKILL.md "New model", step "Sources"**: grep `catalog.csv` (model structure) first, then
  `catalog-code.csv` (programming). Go to the web only if neither has a hit.
- **Web search when both libraries hit**: use the web only for the paper's exact calibration source or a
  specific derivation detail.
- **DSGE_mod is local**: the key DSGE_mod `.mod` files are in `references/examples-code/`. Read them there;
  do not search the web for DSGE_mod.

## Model archive (`model-archive-catalog.csv` + `model-archive/<ModelID>/`)

`references/model-archive-catalog.csv` is a third index, with lower priority than the two libraries. Each
entry has its own folder `references/model-archive/<ModelID>/`. The `Status` column separates two kinds
of entry:

- **`runnable`** (8 entries): models from earlier tasks. The folder holds what a rerun needs (`.mod`,
  derivation note, and where used an external steady-state file, run script, helper functions or
  parameter include).
- **`derivation-only (needs_review)`** (161 entries): MMB paper derivations. The folder has only the
  derivation note (`<ModelID>_derivation.md`) plus
  extraction notes and a source manifest. There is **no `.mod`** and no steady-state or run file. Most are
  first-pass extractions marked `needs_review`: use them as derivation references, not as runnable
  implementations, and check their equations against the paper before they go into Dynare.

Search both ways: grep `model-archive-catalog.csv` for mechanism tags (note `Status`), and list the
folder names in `model-archive/` (skip folders that start with `_`, such as `_mmb-provenance/`; they are
not models). On a hit, read `Status` first: a `runnable` entry can be read and run as a whole folder; a
`derivation-only` entry gives the economic structure only. For a derivation-only `<ModelID>`, also check
`catalog.csv` for `<ModelID>_rep`: 138 of the 161 have a runnable MMB replication in
`references/examples/`.

The archive is read-only. Do not write new entries into the installed skill. Archive a model only when
the user asks, in a location the user chooses (`references/model-archive.md`).
