# Model archive

Read this when you search the model archive (SKILL.md "New model", step "Sources"), or when the user
asks you to archive a finished model.

The archive in the installed skill is a read-only reference. This file gives its two kinds of entry, its
folder layout, how to search it, and how to build an archive for the user in a location the user
chooses.

---

## Two kinds of entry (the `Status` column)

The archive holds two kinds of entry. The **`Status`** column of `model-archive-catalog.csv` separates
them; read it first on every hit.

| `Status` | Content | How to use it |
| -------- | ------- | ------------- |
| `runnable` (8 entries) | Models from earlier modeling tasks: `.mod` plus, where used, steady-state and helper `.m` files | Read the files; the folder can be run as a whole |
| `derivation-only (needs_review)` (161 entries) | MMB paper derivations, consolidated from `mmb-paper-derivations`. **Derivation notes only, no `.mod`, no steady-state file.** Mostly first-pass extractions, not validated by a Dynare run | **Read the derivation for the economic structure** (FOCs, timing, mechanism). Check the equations against the paper before they go into Dynare. Do not treat them as runnable implementations |

## Folder layout

**Each model has its own folder** (folder name = `ModelID`). The two kinds of entry hold different files:

```
references/
├── model-archive-catalog.csv        <- archive index (columns below)
└── model-archive/
    ├── <ModelID>/                    <- runnable entry: every file a rerun needs
    │   ├── <ModelID>.mod            <- Dynare model file
    │   ├── <ModelID>_derivation.md  <- derivation note (optional)
    │   ├── <ModelID>_steadystate.m  <- external steady-state file (required when the steady state is computed in .m)
    │   └── ...                       <- run script, helper functions, parameter include, IRF-matching objective, ...
    ├── <ModelID>/                    <- derivation-only entry: derivation notes only, no .mod
    │   ├── <ModelID>_derivation.md   <- derivation note
    │   ├── extraction_notes.md       <- extraction notes
    │   ├── source_manifest.json      <- paper source manifest (private source path + SHA256, no full text)
    │   └── worker_report.json        <- extraction report (present in 160 of 161 entries)
    └── _mmb-provenance/              <- provenance metadata of the MMB derivations (leading "_" = not a model; skip it)
        ├── metadata/                 <- model_metadata.csv, source_metadata.csv, sha256_manifest.csv, excluded_or_missing.csv, ...
        └── README.md                 <- copyright boundary and snapshot notes
```

**Why a runnable entry is a folder with its `.m` files.** Steady states and coefficients are often
computed in `.m` files: an external `<ModelID>_steadystate.m` that back-solves the calibration, a
`fzero` for a coefficient (for example the BGG financial-accelerator coefficients in
`swff/swff_ff_coeffs.m`), an IRF-matching objective. A `.mod` and a derivation note alone may not rerun.
One complete folder can be copied and run as it is.

**In a runnable entry, the `.mod` is the model that runs.** Its derivation note was written before or
during the work and can disagree with the final `.mod`: a parameter value, a variable name, a sign, the
scope of the model. When the two differ, follow the `.mod` (and its `.m` files), and check the note
against the paper before you reuse its equations.

**Why the derivation-only entries are useful.** They are equation-by-equation derivations of MMB papers
in the eight-section structure (`references/derivation-style.md`). Even without a `.mod` they give the
FOCs, timing and calibration of the paper in ready form. `needs_review` is an honest flag: use them as
references, not as trusted implementations.

**Read `<ModelID>_derivation.md`.**

## Columns of `model-archive-catalog.csv`

`ModelID, Task, Paper, Year, ModelType, Economy, Category, KeyFeatures, DateAdded, Status`

The columns follow `catalog.csv` without `Authors` and `Journal`, plus `Task`, `DateAdded` and `Status`,
so one grep pattern works on both files.

| Column | Meaning |
|--------|---------|
| `ModelID` | Model identifier = folder name = `.mod` file name without `.mod` |
| `Task` | One-sentence summary of the task that produced the entry (separate from the paper description) |
| `Paper` | Full title of the replicated paper; `custom` for a model built for a task |
| `Year` | Year of the paper; for a custom model, the year it was built |
| `ModelType` | Model family and whether it is linearized, for example `NK nonlinear`, `RBC nonlinear` |
| `Economy` | `Closed economy`, `Open economy`, `Euro Area`, … |
| `Category` | One of the 14 buckets in `references/catalog-lookup.md`, for example `3. Financial Accelerator / BGG-type Credit Frictions`. Two runnable entries use other labels (`6. Financial Frictions / Credit Frictions`, `12. Growth / Demographics / OLG`), so grep `KeyFeatures` as well |
| `KeyFeatures` | Free-text mechanism tags for grep, for example `financial accelerator, Calvo pricing, @#define switch` |
| `DateAdded` | Date the entry was added, `YYYY-MM-DD` |
| `Status` | `runnable` (has a `.mod` and reruns) or `derivation-only (needs_review)` (derivation reference only, no `.mod`) |

Example rows (format; the shipped rows may differ):

```csv
"ModelID","Task","Paper","Year","ModelType","Economy","Category","KeyFeatures","DateAdded","Status"
"bgg_financial","BGG 1999 financial accelerator vs frictionless NK; @#define macro switch WITH_FA","Bernanke Gertler Gilchrist 1999","1999","NK nonlinear","Closed economy","3. Financial Accelerator / BGG-type Credit Frictions","financial accelerator, CSV optimal contract, external finance premium, entrepreneur net worth, @#ifndef WITH_FA macro switch, Calvo nonlinear pricing, CEE investment adjustment costs","2026-06-13","runnable"
"EA_SW03","MMB paper derivation reference (8-section FOC/equilibrium derivation, EN+ZH); no runnable .mod","An Estimated Stochastic Dynamic General Equilibrium Model of the Euro Area","2003","New Keynesian DSGE, medium-scale, Bayesian estimation","Euro Area","2. Estimated DSGE Benchmarks (Smets-Wouters Type)","Calvo prices, Calvo wages, habit formation, investment adj. costs, variable capital util.","2026-06-18","derivation-only (needs_review)"
```

---

## 1. Search (SKILL.md "New model", step "Sources")

Search the archive after `catalog.csv` (149 MMB models) has no close hit. **Search both ways**: grep the
catalog text, and list the **folder names** (folder = `ModelID`; the mechanism is sometimes visible in
the name, and the folder list also catches entries that a catalog row describes badly):

```bash
# grep the catalog (mechanism tags, economy and category are all here)
grep -iE "financial accelerator|BGG|spread" references/model-archive-catalog.csv
grep -iE "TANK|hand-to-mouth"               references/model-archive-catalog.csv

# list the folder names (ModelID) and match common model-family abbreviations
ls references/model-archive/ | grep -iE "swff|bgg|hank|rbc|nk"
```

If `references/model-archive-catalog.csv` or `references/model-archive/` does not exist, skip the
archive. Treat it as no hit; it is not an error.

On a hit, **read `Status` first**, then open `references/model-archive/<ModelID>/`:

- `runnable`: read the `.mod` (and, where needed, `<ModelID>_steadystate.m` and the other `.m` files) as
  an additional reference. Take content, timing and calibration; do not copy the form unchanged.
- `derivation-only (needs_review)`: there is no `.mod`. Read `<ModelID>_derivation.md` for the FOC
  structure, timing convention, mechanism and calibration hints. These are unvalidated first-pass
  extractions: check every equation against the paper before it goes into Dynare. Also check
  `catalog.csv` for `<ModelID>_rep`: 138 of the 161 entries have a runnable MMB replication in
  `references/examples/`.

---

## 2. Archive a model (only when the user asks)

Do not write into the installed skill: do not copy models into `references/model-archive/` and do not
append rows to `references/model-archive-catalog.csv`. Do not ask an archive question at the end of a
task.

When the user asks you to archive a model, ask where the archive goes unless they said so, then reuse
the layout above in that location:

```
<archive-root>/
├── model-archive-catalog.csv
└── model-archive/
    └── <ModelID>/
```

**a. Create the model folder** (folder name = `ModelID`; create the parents if needed):
```bash
mkdir -p <archive-root>/model-archive/<ModelID>
```

**b. Copy every file the model needs to rerun:**
```bash
cp <work-dir>/<ModelID>.mod              <archive-root>/model-archive/<ModelID>/
# derivation note, if the task produced one
cp <work-dir>/<ModelID>_derivation.md    <archive-root>/model-archive/<ModelID>/
# steady-state and helper .m files: when the steady state or key coefficients are computed in .m,
# copy them, or the model will not run:
#   external steady-state file <ModelID>_steadystate.m, run script (for example run_<ModelID>.m),
#   helper functions (coefficient solvers, IRF-matching objective, ...), parameter includes (*.inc), ...
cp <work-dir>/<ModelID>_steadystate.m    <archive-root>/model-archive/<ModelID>/   # if it exists
cp <work-dir>/run_<ModelID>.m            <archive-root>/model-archive/<ModelID>/   # if it exists
# copy any other .m / .inc / small data file that this model needs to rerun
```

> Test: **can the folder alone compute the steady state and the IRFs from scratch?** If not, add the
> missing files.
> Do not copy: files that Dynare generates (`+<ModelID>/` with `driver.m`, `dynamic.m`, `static.m`, …;
> the `<ModelID>/` output folder with `Output/<ModelID>_results.mat`), paper PDFs, or the skill's plotting
> helpers (`references/plot_irfs_pub.m`, `references/plot_series_pub.m`). If the run script calls the
> plotting helpers, say in `Task` that they must be on the MATLAB/Octave path; copy them only if the user
> wants the archive to run without the skill.

**c. Create the CSV header** (only if `<archive-root>/model-archive-catalog.csv` does not exist):
```
"ModelID","Task","Paper","Year","ModelType","Economy","Category","KeyFeatures","DateAdded","Status"
```

**d. Append one catalog row** with the columns described above. A model from a task gets
`Status` = `runnable`.

**e. Report**: tell the user where the model was archived
(`<archive-root>/model-archive/<ModelID>/`) and list the files you copied, so the user can check that
everything a rerun needs is there.
