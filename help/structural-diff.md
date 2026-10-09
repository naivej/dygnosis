# Semantic model Diff

Open a model and run **Dygnosis: Open changes**. The open model is **After**.
Choose its **Before** version:

1. **With previous revision** uses HEAD for a Working model, or the first parent
   of the commit opened in a historical document. The picker names that commit.
2. **With revision…** lists repository commits, offers Load more, and accepts a
   local revision such as `HEAD~2` or a commit hash.
3. **With branch or tag…** selects the fixed commit of a local branch,
   remote-tracking branch, or tag.
4. **With .mod file…** selects another current `.mod` or `.dyn` model.

![Focused review in VS Code with a changed local definition, field detail and direct equation references](assets/semantic-diff.png)

**Working** means current saved files plus open unsaved text. A historical model
uses its root and active includes from the selected commit. The four choices
keep the invoking model version as After, even when HEAD is elsewhere. On an
include, choose its model owner when asked. An unknown historical owner is
checked at that same commit. The Source Control action uses its selected
Working model, including when invoked from the staged list.

Git choices remain visible when unavailable and explain the reason. File
comparison stays available. **Open changes with .mod file…** is the direct
file-comparison command for existing bindings. Cancelling a picker keeps the
current comparison.

The top bar keeps Before, After and Presentation together. Use **More actions**
for Root file text diff, Update revision, Details, Help and Expansion. Details
shows the full captured input paths and include settings.

The Changes tab names both inputs. **Change comparison…** chooses another
baseline for the original open model. **Swap sides** exchanges Before and After,
including the direction of Added and Removed. **Refresh** keeps selected commits
fixed; **Update revision** resolves their requested refs again. Git reads do not
fetch, check out files, or change the index. Missing local objects require the
user to obtain them before retrying. A missing root offers **Choose model path**;
an established rename needs a path choice before it is used.

For history comparisons, both sides use the launching model's current
[extra include folders](settings:dynare.searchPaths). Details shows the captured
folders. Each historical side uses its own written `@#includepath` and macros.
Historical settings files are not applied automatically. Active historical
includes outside the repository, through symlinks, or across submodules cannot
be compared. A model rooted inside a checked-out submodule uses its own
repository. File comparisons use each current root's own include settings.

## Review the model

The Changes view opens on **Model changes**. It compares retained model facts
across supported parser families, including symbols, parameters, equations,
priors, commands and other accepted settings. One changed fact has one model
row. Supporting statement context and equation references do not add more model
changes. Model counts describe displayed row appearances; Source counts name
captured files and text hunks separately.

**Focused review** is the default presentation. It shows a compact list beside
the selected detail. **Change list** shows expandable summaries. The
Presentation switch stays beside Before and After on Model changes, Source
changes and Coverage. Both presentations use the same comparison. Each keeps
its own filters, selection, detail layout and expansion state when you switch.
Each split keeps its own state while sharing the captured comparison.

Details show named Before and After fields, with absent, empty and unknown values
kept distinct. Equation expressions highlight changed identifiers, timing
suffixes, operators and constants while keeping unchanged context readable.
Timing shows written offsets and, where `predetermined_variables` applies,
offsets after convention conversion. Details can list direct written references
to changed symbols and parameters, including references in unchanged equations.
References are not claims about indirect dependencies or numerical effects.
Uncertain occurrences remain separate and are labeled Unpaired; text-only
highlights do not imply that two equations were paired. The comparison does not
solve the model or report simulation, estimation or steady-state results.

Find changes, Kind, Section and Scope filter model rows. Kind and Section offer
All or one choice; a saved group of choices remains available as a saved
selection. **Detail** supports Auto, Side by side and Stacked. When no model row
is available, check Coverage and
Source changes before concluding that the written files are unchanged.

**Open source** opens the row's verified written source on that side. Direct
reference actions open the referenced equation. Several contributing files
offer a source picker; an absent or unverified target has a disabled action
and a reason. Historical sources are read-only and retain their commit and
captured text after the Changes tab closes.

## Source changes and coverage

**Source changes** compares normalized text from the selected roots and their
captured executed includes. It keeps comments, formatting, macro text and edits
that also have semantic rows. Roots pair because you selected them. Includes
pair only when their captured identity is proven; an uncertain file stays
separate. The view does not guess renames from filenames or similar text.

Select a captured file to see its hunks and line numbers. **Open Before captured
text** and **Open After captured text** show read-only text retained by the
comparison. **Captured file text diff** opens the two retained sides in VS Code.
If alignment reaches its limit, the view reports omitted hunks and keeps this
text-diff action when the captured text is complete. Hunk positions are not
source-navigation targets. **Root file text diff** remains available for the
written roots. It opens VS Code's text diff for the selected roots; an
include-only edit may have no root-file hunk.

**Coverage** lists compared families, available fields, limits and the source
boundary. Coverage is field-specific: a limit on one field does not hide other
facts the parser retained. If an occurrence cannot be matched safely, the view
keeps the useful side facts and states the limit. When a comparison budget
prevents a field comparison, that field stays plain and is not presented as a
change; other available fields still compare.

Only captured roots and executed includes are in the Source comparison.
Unexecuted child files, external MATLAB functions and data-file contents are
not captured. In supplied-text mode, only the supplied roots and include text
are available. An empty semantic result does not mean that all source text is
unchanged. When source capture is incomplete, Coverage states the boundary and
does not claim a complete source comparison.

## Refresh and settings

A change to a Working root, active include, missing candidate or relevant
include setting marks the comparison **Out of date**. Rows remain visible, but
source actions are disabled until Refresh captures current inputs. Source
actions also check that their retained target still belongs to the current
capture. Unrelated edits do not invalidate it. Two fixed historical inputs
remain current across Working edits. An engine restart requires a new capture.
An incomplete or failed refresh clears authoritative comparison rows.

[Presentation](settings:dynare.diff.presentation) defaults to Focused review;
choose Change list to make that the initial view for new comparisons.
[Diff layout](settings:dynare.diff.layout) keeps Auto, Side by side and Stacked.
Changing `dynare.diff.sections` updates open comparisons. Other defaults apply
when opening a comparison. Display choices survive Refresh and window reload;
restored inputs are captured again before source actions become available.
Missing repositories, commits and untitled inputs have an explicit failure
state. History comparison requires snapshot support in the engine; an older
engine can still support current-file comparison.
