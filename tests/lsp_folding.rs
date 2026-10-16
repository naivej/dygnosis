//! Written folding must survive unavailable macro values/includes and avoid
//! borrowing a delimiter from another file, branch, string, or native line.

use dygnosis::server::new_service;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

fn uri(name: &str) -> Url {
    Url::parse(&format!("file:///C:/dygnosis-folding/{name}")).unwrap()
}

async fn open(server: &dygnosis::server::Backend, uri: &Url, text: &str) {
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}

async fn ranges(server: &dygnosis::server::Backend, uri: &Url) -> Vec<FoldingRange> {
    server
        .folding_range(FoldingRangeParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap_or_default()
}

async fn folds(text: &str) -> Vec<(u32, u32)> {
    let (service, _socket) = new_service();
    let file = uri("root.mod");
    open(service.inner(), &file, text).await;
    ranges(service.inner(), &file)
        .await
        .iter()
        .map(|range| (range.start_line, range.end_line))
        .collect()
}

#[tokio::test]
async fn multiline_statements_close_at_their_own_semicolon() {
    let text = "var\ny\nc\n;\nvarexo\ne\n;\nparameters\np\n;\np =\n0.5\n;\nstoch_simul(\norder=1,\nirf=10\n) y c;\nmodel(linear);\ny = p*c;\nc = e;\nend;\n";
    assert_eq!(
        folds(text).await,
        vec![(0, 3), (4, 6), (7, 9), (10, 12), (13, 16), (17, 20)]
    );
    assert!(folds("var y;\nvar c;\nparameters p;\n").await.is_empty());
}

#[tokio::test]
async fn nested_macro_and_branch_folds_do_not_hide_the_next_directive() {
    let text = "@#if UNKNOWN\nvar\ny;\n@#elseif OTHER\nparameters\np;\n@#else\n@#for co in COUNTRIES\nvar GDP_@{co}\nC_@{co};\n@#endfor\n@#endif\n";
    assert_eq!(
        folds(text).await,
        vec![(0, 11), (1, 2), (3, 5), (4, 5), (6, 10), (7, 10), (8, 9)]
    );
}

#[tokio::test]
async fn inactive_blocks_and_failed_macro_evaluation_keep_written_boundaries() {
    let text = "@#if 0\nmodel;\ny=0;\nend;\n@#else\nmodel(linear);\ny=1;\nend;\n@#endif\n@#error \"stop\"\nparameters\np;\n";
    assert_eq!(
        folds(text).await,
        vec![(0, 8), (1, 3), (4, 7), (5, 7), (10, 11)]
    );
}

#[tokio::test]
async fn repeated_macro_expansion_has_one_written_fold_per_start_line() {
    let text = "@#for co in [\"H\", \"F\"]\nvar\nGDP_@{co};\n@#endfor\nmodel;\n@#for co in [\"H\", \"F\"]\nGDP_@{co}=0;\n@#endfor\nend;\n";
    assert_eq!(folds(text).await, vec![(0, 3), (1, 2), (4, 8), (5, 7)]);
}

#[tokio::test]
async fn conditional_variants_and_continued_directives_use_physical_lines() {
    for keyword in ["if", "ifdef", "ifndef"] {
        let text = format!("@#{keyword} UNKNOWN\nvar y;\nvar c;\n@#endif\n");
        assert_eq!(folds(&text).await, vec![(0, 3), (1, 2)], "{keyword}");
    }
    let text = "@#if UNKNOWN \\\\\n || OTHER\nvar y;\nvar c;\n@#else\nvar z;\n@#endif\n";
    assert_eq!(folds(text).await, vec![(0, 6), (2, 3), (4, 5)]);
    assert_eq!(
        folds(&text.replace('\n', "\r\n")).await,
        vec![(0, 6), (2, 3), (4, 5)]
    );
}

#[tokio::test]
async fn lexical_delimiter_text_and_native_indexing_do_not_make_false_folds() {
    let text = "var\ny (long_name='end; /*')\nc $C;$\n;\n// model;\n% end;\ndisp('/*');\nhelper = data(end);\n/* real\ncomment\n*/\nmodel;\ny=c;\nend;\n";
    let (service, _socket) = new_service();
    let file = uri("lexical.mod");
    open(service.inner(), &file, text).await;
    let actual = ranges(service.inner(), &file).await;
    assert_eq!(
        actual
            .iter()
            .map(|row| (row.start_line, row.end_line))
            .collect::<Vec<_>>(),
        vec![(0, 3), (8, 10), (11, 13)]
    );
    assert_eq!(actual[1].kind, Some(FoldingRangeKind::Comment));
    assert!(folds("helper = [\n1, 2\n];\n").await.is_empty());
}

#[tokio::test]
async fn incomplete_constructs_recover_later_complete_source_without_borrowing_closers() {
    assert_eq!(
        folds("var y\nparameters\np;\nmodel;\ny=0;\n").await,
        vec![(1, 2)]
    );
    assert_eq!(
        folds("@#if UNKNOWN\nmodel;\ny=0;\n@#else\nend;\n@#endif\nparameters\np;\n").await,
        vec![(0, 5), (1, 2), (3, 4), (6, 7)]
    );
    assert_eq!(
        folds("model;\ny=;\nend;\nparameters\np;\n").await,
        vec![(0, 2), (3, 4)]
    );
    assert_eq!(
        folds("model;\ny=0;\nparameters\np;\nshocks;\nvar e;\nstderr 1;\nend;\n").await,
        vec![(2, 3), (4, 7)]
    );
    assert_eq!(
        folds("@#if UNKNOWN\n@#for co in UNKNOWN\nvar\ny_@{co};\n@#endfor\nparameters\np;\n").await,
        vec![(1, 4), (2, 3), (5, 6)]
    );
}

#[tokio::test]
async fn unresolved_include_does_not_remove_same_file_folds_or_join_separate_files() {
    let (service, _socket) = new_service();
    let server = service.inner();
    let root = uri("include.mod");
    open(
        server,
        &root,
        "var y;\nmodel;\n@#include \"missing.inc\"\nend;\n",
    )
    .await;
    assert_eq!(
        ranges(server, &root)
            .await
            .iter()
            .map(|row| (row.start_line, row.end_line))
            .collect::<Vec<_>>(),
        vec![(1, 3)]
    );
    let child = uri("closer.inc");
    open(server, &child, "y=0;\nend;\n").await;
    open(server, &root, "var y;\nmodel;\n@#include \"closer.inc\"\n").await;
    assert!(ranges(server, &root).await.is_empty());
    assert!(ranges(server, &child).await.is_empty());
}
