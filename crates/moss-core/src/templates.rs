//! Starter documents for new Projects: each is a small, real, compiling document of a common kind
//! (article, IEEE paper, report, thesis, slides, letter, CV, XeLaTeX), with the structure people
//! expect so an agent or a person can start writing instead of assembling boilerplate.
//!
//! They use only packages that ship with MiKTeX and TeX Live.

pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// The file to compile.
    pub root_file: &'static str,
    /// `(path, content)`; folders are created as needed.
    pub files: &'static [(&'static str, &'static str)],
}

pub fn all() -> &'static [Template] {
    TEMPLATES
}

pub fn find(id: &str) -> Option<&'static Template> {
    let id = id.trim().to_ascii_lowercase();
    TEMPLATES.iter().find(|t| t.id == id)
}

const TEMPLATES: &[Template] = &[
    Template {
        id: "article",
        name: "Article",
        description: "A short paper or essay: title, abstract, sections, an equation, a table and a list.",
        root_file: "main.tex",
        files: &[("main.tex", ARTICLE)],
    },
    Template {
        id: "paper",
        name: "Research paper (IEEE, two columns)",
        description: "A conference paper in the IEEEtran class with abstract, keywords, a figure, a table and a BibTeX bibliography.",
        root_file: "main.tex",
        files: &[("main.tex", PAPER), ("refs.bib", REFS)],
    },
    Template {
        id: "report",
        name: "Report",
        description: "A report with a title page, contents, chapters in separate files, a bibliography and an appendix.",
        root_file: "main.tex",
        files: &[
            ("main.tex", REPORT),
            ("chapters/introduction.tex", REPORT_INTRO),
            ("chapters/method.tex", REPORT_METHOD),
            ("chapters/results.tex", REPORT_RESULTS),
            ("chapters/conclusion.tex", REPORT_CONCLUSION),
            ("chapters/appendix.tex", REPORT_APPENDIX),
            ("refs.bib", REFS),
        ],
    },
    Template {
        id: "thesis",
        name: "Thesis or book",
        description: "A book-class thesis with front matter (abstract, contents, lists of figures and tables), chapters, a natbib bibliography and an appendix.",
        root_file: "main.tex",
        files: &[
            ("main.tex", THESIS),
            ("frontmatter/abstract.tex", THESIS_ABSTRACT),
            ("chapters/introduction.tex", THESIS_INTRO),
            ("chapters/background.tex", THESIS_BACKGROUND),
            ("chapters/conclusion.tex", THESIS_CONCLUSION),
            ("refs.bib", REFS),
        ],
    },
    Template {
        id: "beamer",
        name: "Slides (Beamer)",
        description: "A presentation: title slide, outline, bullet and two-column slides, a block and a closing slide.",
        root_file: "main.tex",
        files: &[("main.tex", BEAMER)],
    },
    Template {
        id: "letter",
        name: "Letter",
        description: "A formal letter with sender, recipient, opening and closing.",
        root_file: "main.tex",
        files: &[("main.tex", LETTER)],
    },
    Template {
        id: "cv",
        name: "CV / resume",
        description: "A one-page CV with sections for experience, education and skills.",
        root_file: "main.tex",
        files: &[("main.tex", CV)],
    },
    Template {
        id: "unicode",
        name: "Unicode text (XeLaTeX)",
        description: "A document compiled with XeLaTeX and fontspec, with accented letters, symbols and ligatures.",
        root_file: "main.tex",
        files: &[("main.tex", UNICODE)],
    },
    Template {
        id: "tour",
        name: "Moss tour",
        description: "A short guided document with maths, a figure, a table, a citation and a cross-reference, with comments that point at what Moss can do.",
        root_file: "main.tex",
        files: &[("main.tex", TOUR), ("refs.bib", REFS)],
    },
];

const TOUR: &str = r#"\documentclass[11pt,a4paper]{article}
\usepackage[margin=2.5cm]{geometry}
\usepackage{amsmath}
\usepackage{booktabs}
\usepackage{xcolor}
\usepackage[hidelinks]{hyperref}

\title{Welcome to the Moss tour}
\author{You}
\date{\today}

\begin{document}
\maketitle

% Press Ctrl+Enter (or the Compile button) to build this file. The PDF appears on the right.
% Ctrl+click a spot in the PDF to jump to its source line, and the other way round.

\section{Writing}
Moss edits plain \LaTeX{} files on your PC. Switch the editor between \emph{Code} and \emph{Visual}
at the top; the file on disk stays exactly what you typed.

\section{Maths}\label{sec:maths}
Inline maths such as $e^{i\pi}+1=0$ sits in the text, and display maths gets its own line:
\begin{equation}\label{eq:gauss}
  \int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}.
\end{equation}
Equation~\ref{eq:gauss} is a cross-reference. Rename the label and Moss shows what broke.

\section{A figure and a table}
\begin{figure}[ht]
  \centering
  \fbox{\textcolor{blue!60!black}{\rule{0pt}{3cm}\hspace{6cm}}}
  \caption{A placeholder figure. Drop an image into the project and use \texttt{\textbackslash includegraphics}.}
  \label{fig:placeholder}
\end{figure}

\begin{table}[ht]
  \centering
  \begin{tabular}{lrr}
    \toprule
    Feature & Where & Try it \\
    \midrule
    Compile & top bar & Ctrl+Enter \\
    Search & sidebar & Ctrl+Shift+F \\
    History & clock icon & restore anything \\
    \bottomrule
  \end{tabular}
  \caption{A few things to try.}
\end{table}

\section{Citing}
Literate programming~\cite{knuth1984} and the \LaTeX{} manual~\cite{lamport1994} come from
\texttt{refs.bib}. Section~\ref{sec:maths} and Figure~\ref{fig:placeholder} are references too.

\section{What next}
Link this project to Overleaf from the sync button, or let an AI agent edit it through
\texttt{moss-mcp}: every change it makes can be undone.

\bibliographystyle{plain}
\bibliography{refs}
\end{document}
"#;

const ARTICLE: &str = r#"\documentclass[11pt,a4paper]{article}
\usepackage[margin=2.5cm]{geometry}
\usepackage{amsmath}
\usepackage{booktabs}
\usepackage[hidelinks]{hyperref}

\title{A Short Article}
\author{Your Name}
\date{\today}

\begin{document}
\maketitle

\begin{abstract}
One paragraph that says what this article is about, what was done, and what was found.
\end{abstract}

\section{Introduction}
\label{sec:intro}
Start here. State the question and why it matters. Section~\ref{sec:method} describes the approach and Section~\ref{sec:results} the results.

\section{Method}
\label{sec:method}
The quantity of interest is defined in Equation~\eqref{eq:mean}:
\begin{equation}
  \bar{x} = \frac{1}{n}\sum_{i=1}^{n} x_i .
  \label{eq:mean}
\end{equation}
Things to remember:
\begin{itemize}
  \item write one idea per paragraph;
  \item define every symbol the first time it appears;
  \item keep the figures and tables referred to in the text.
\end{itemize}

\section{Results}
\label{sec:results}
Table~\ref{tab:results} summarises the measurements.
\begin{table}[ht]
  \centering
  \caption{Example measurements.}
  \label{tab:results}
  \begin{tabular}{lrr}
    \toprule
    Case & Mean & Std.\ dev. \\
    \midrule
    A & 1.20 & 0.10 \\
    B & 1.85 & 0.22 \\
    C & 2.40 & 0.31 \\
    \bottomrule
  \end{tabular}
\end{table}

\section{Conclusion}
Say what the reader should take away, and what remains to be done.

\end{document}
"#;

const REFS: &str = r#"@article{knuth1984,
  author  = {Donald E. Knuth},
  title   = {Literate Programming},
  journal = {The Computer Journal},
  volume  = {27},
  number  = {2},
  pages   = {97--111},
  year    = {1984}
}

@book{lamport1994,
  author    = {Leslie Lamport},
  title     = {{LaTeX}: A Document Preparation System},
  publisher = {Addison-Wesley},
  year      = {1994}
}

@inproceedings{shannon1948,
  author    = {Claude E. Shannon},
  title     = {A Mathematical Theory of Communication},
  booktitle = {Bell System Technical Journal},
  year      = {1948}
}
"#;

const PAPER: &str = r#"\documentclass[conference]{IEEEtran}
\usepackage{amsmath}
\usepackage{graphicx}
\usepackage{tikz}
\usepackage{booktabs}
\usepackage{cite}

\begin{document}

\title{A Conference Paper Title}

\author{\IEEEauthorblockN{First Author}
\IEEEauthorblockA{Department of Engineering\\
Example University\\
first.author@example.edu}
\and
\IEEEauthorblockN{Second Author}
\IEEEauthorblockA{Department of Computer Science\\
Example University\\
second.author@example.edu}}

\maketitle

\begin{abstract}
This paper presents an approach to the problem, evaluates it on a set of cases, and shows that it improves on the baseline. The abstract should stand on its own in two hundred words or fewer.
\end{abstract}

\begin{IEEEkeywords}
template, research paper, LaTeX
\end{IEEEkeywords}

\section{Introduction}
Documents are prepared with a markup language~\cite{lamport1994}, and good tools make that practical~\cite{knuth1984}. The remainder of this paper is organised as follows. Section~\ref{sec:method} describes the method and Section~\ref{sec:results} reports results.

\section{Method}
\label{sec:method}
The information content of a source is measured by its entropy~\cite{shannon1948}:
\begin{equation}
  H(X) = -\sum_{i} p_i \log_2 p_i .
  \label{eq:entropy}
\end{equation}
Figure~\ref{fig:flow} shows the processing pipeline.

\begin{figure}[t]
  \centering
  \begin{tikzpicture}[node distance=2.2cm, every node/.style={draw, rounded corners, minimum width=1.6cm, minimum height=0.8cm, font=\small}]
    \node (a) {Input};
    \node (b) [right of=a] {Model};
    \node (c) [right of=b] {Output};
    \draw[->] (a) -- (b);
    \draw[->] (b) -- (c);
  \end{tikzpicture}
  \caption{The processing pipeline.}
  \label{fig:flow}
\end{figure}

\section{Results}
\label{sec:results}
Table~\ref{tab:acc} compares the method with the baseline.

\begin{table}[t]
  \caption{Accuracy on the test set.}
  \label{tab:acc}
  \centering
  \begin{tabular}{lcc}
    \toprule
    Method & Accuracy (\%) & Time (s) \\
    \midrule
    Baseline & 81.2 & 12.4 \\
    Proposed & 88.7 & 9.8 \\
    \bottomrule
  \end{tabular}
\end{table}

\section{Conclusion}
The proposed method is more accurate and faster than the baseline. Future work will extend it to larger inputs.

\bibliographystyle{IEEEtran}
\bibliography{refs}

\end{document}
"#;

const REPORT: &str = r#"\documentclass[12pt,a4paper]{report}
\usepackage[margin=2.5cm]{geometry}
\usepackage{amsmath,amssymb}
\usepackage{graphicx}
\usepackage{tikz}
\usepackage{booktabs}
\usepackage[hidelinks]{hyperref}

\begin{document}

\begin{titlepage}
  \centering
  \vspace*{3cm}
  {\Huge\bfseries Project Report\par}
  \vspace{1cm}
  {\Large A study of an interesting problem\par}
  \vspace{2cm}
  {\large Your Name\par}
  {\large Department of Engineering\par}
  \vfill
  {\large \today\par}
\end{titlepage}

\tableofcontents
\listoffigures
\listoftables

\include{chapters/introduction}
\include{chapters/method}
\include{chapters/results}
\include{chapters/conclusion}

\bibliographystyle{unsrt}
\bibliography{refs}

\appendix
\include{chapters/appendix}

\end{document}
"#;

const REPORT_INTRO: &str = r#"\chapter{Introduction}
\label{ch:intro}

\section{Background}
Explain the setting of the work and who needs it. Earlier work~\cite{knuth1984} established the idea, and~\cite{lamport1994} made it practical.

\section{Objectives}
The report has three objectives:
\begin{enumerate}
  \item to define the problem precisely;
  \item to describe and implement a solution (Chapter~\ref{ch:method});
  \item to evaluate it (Chapter~\ref{ch:results}).
\end{enumerate}

\section{Outline}
Chapter~\ref{ch:method} gives the method, Chapter~\ref{ch:results} the results, and Chapter~\ref{ch:conclusion} the conclusions.
"#;

const REPORT_METHOD: &str = r#"\chapter{Method}
\label{ch:method}

\section{Approach}
The approach follows the pipeline in Figure~\ref{fig:pipeline}.

\begin{figure}[ht]
  \centering
  \begin{tikzpicture}[node distance=2.4cm, every node/.style={draw, rounded corners, minimum width=1.8cm, minimum height=0.8cm}]
    \node (a) {Collect};
    \node (b) [right of=a] {Process};
    \node (c) [right of=b] {Report};
    \draw[->] (a) -- (b);
    \draw[->] (b) -- (c);
  \end{tikzpicture}
  \caption{The pipeline used in this work.}
  \label{fig:pipeline}
\end{figure}

\section{Model}
The quantity computed at each step is
\begin{equation}
  y = \alpha x + \beta, \qquad \alpha, \beta \in \mathbb{R}.
  \label{eq:model}
\end{equation}
"#;

const REPORT_RESULTS: &str = r#"\chapter{Results}
\label{ch:results}

\section{Measurements}
Table~\ref{tab:measurements} lists what was measured using Equation~\eqref{eq:model}.

\begin{table}[ht]
  \centering
  \caption{Measured values.}
  \label{tab:measurements}
  \begin{tabular}{lrrr}
    \toprule
    Run & $x$ & $y$ & Error (\%) \\
    \midrule
    1 & 1.0 & 2.1 & 1.2 \\
    2 & 2.0 & 4.0 & 0.8 \\
    3 & 3.0 & 6.2 & 1.5 \\
    \bottomrule
  \end{tabular}
\end{table}

\section{Discussion}
The error stays below two percent in every run, which meets the objective set in Chapter~\ref{ch:intro}.
"#;

const REPORT_CONCLUSION: &str = r#"\chapter{Conclusion}
\label{ch:conclusion}

The work met its objectives. The method is simple, its results are repeatable, and the limits are understood. Further work could extend the method to larger data sets.
"#;

const REPORT_APPENDIX: &str = r#"\chapter{Raw Data}
\label{app:data}

Long tables, derivations and code listings that would interrupt the main text belong here.
"#;

const THESIS: &str = r#"\documentclass[12pt,a4paper,oneside]{book}
\usepackage[margin=3cm]{geometry}
\usepackage{amsmath,amssymb}
\usepackage{graphicx}
\usepackage{tikz}
\usetikzlibrary{positioning}
\usepackage{booktabs}
\usepackage[numbers]{natbib}
\usepackage[hidelinks]{hyperref}

\begin{document}

\frontmatter
\begin{titlepage}
  \centering
  \vspace*{2.5cm}
  {\Huge\bfseries Thesis Title\par}
  \vspace{1.5cm}
  {\Large A thesis submitted for the degree of\\ Master of Science\par}
  \vspace{2cm}
  {\large Your Name\par}
  \vfill
  {\large Example University\\ \today\par}
\end{titlepage}

\input{frontmatter/abstract}
\tableofcontents
\listoffigures
\listoftables

\mainmatter
\input{chapters/introduction}
\input{chapters/background}
\input{chapters/conclusion}

\appendix
\chapter{Supplementary Material}
\label{app:extra}
Extra derivations and data go here.

\backmatter
\clearpage
\addcontentsline{toc}{chapter}{Bibliography}
\bibliographystyle{plainnat}
\bibliography{refs}

\end{document}
"#;

const THESIS_ABSTRACT: &str = r#"\chapter*{Abstract}
\addcontentsline{toc}{chapter}{Abstract}
This thesis studies a problem, proposes a solution, and evaluates it. The abstract should summarise the question, the approach, the main results and their significance in about one page.
"#;

const THESIS_INTRO: &str = r#"\chapter{Introduction}
\label{ch:intro}

\section{Motivation}
Why the problem matters, with the first references~\citep{knuth1984,lamport1994}.

\section{Contributions}
The main contributions are:
\begin{itemize}
  \item a precise statement of the problem (Chapter~\ref{ch:background});
  \item a method that solves it;
  \item an evaluation that supports the claims.
\end{itemize}

\section{Structure of the thesis}
Chapter~\ref{ch:background} gives the background and Chapter~\ref{ch:conclusion} concludes.
"#;

const THESIS_BACKGROUND: &str = r#"\chapter{Background}
\label{ch:background}

\section{Definitions}
Let $X$ be a random variable with distribution $p$. Its entropy~\citep{shannon1948} is
\begin{equation}
  H(X) = -\sum_{x} p(x)\log p(x).
  \label{eq:entropy}
\end{equation}

\section{Related work}
Earlier work is grouped by theme. Figure~\ref{fig:taxonomy} sorts the approaches and Table~\ref{tab:related} compares them.

\begin{figure}[ht]
  \centering
  \begin{tikzpicture}[every node/.style={draw, rounded corners, minimum height=0.8cm, font=\small}]
    \node (root) {Approaches};
    \node (a) [below left=0.9cm and 0.4cm of root] {Exact};
    \node (b) [below right=0.9cm and 0.4cm of root] {Approximate};
    \draw (root) -- (a);
    \draw (root) -- (b);
  \end{tikzpicture}
  \caption{A simple taxonomy of the approaches.}
  \label{fig:taxonomy}
\end{figure}

\begin{table}[ht]
  \centering
  \caption{Comparison of related approaches.}
  \label{tab:related}
  \begin{tabular}{lcc}
    \toprule
    Approach & Scales & Exact \\
    \midrule
    Method A & yes & no \\
    Method B & no & yes \\
    \bottomrule
  \end{tabular}
\end{table}
"#;

const THESIS_CONCLUSION: &str = r#"\chapter{Conclusion}
\label{ch:conclusion}

Summarise the findings, state the limits of the work, and propose directions for the future.
"#;

const BEAMER: &str = r#"\documentclass[aspectratio=169]{beamer}
\usetheme{Madrid}
\usepackage{amsmath}

\title{A Short Presentation}
\subtitle{With a subtitle}
\author{Your Name}
\institute{Example University}
\date{\today}

\begin{document}

\begin{frame}
  \titlepage
\end{frame}

\begin{frame}{Outline}
  \tableofcontents
\end{frame}

\section{Motivation}
\begin{frame}{Why this matters}
  \begin{itemize}
    \item The problem affects many people.
    \item Existing solutions are slow.
    \item We propose a faster one.
  \end{itemize}
\end{frame}

\section{Approach}
\begin{frame}{The idea}
  \begin{columns}
    \column{0.5\textwidth}
      A simple model:
      \[ y = \alpha x + \beta \]
    \column{0.5\textwidth}
      \begin{block}{Key point}
        One equation is enough to explain the result.
      \end{block}
  \end{columns}
\end{frame}

\section{Results}
\begin{frame}{Results}
  \begin{tabular}{lcc}
    Method & Accuracy & Time \\ \hline
    Baseline & 81\% & 12 s \\
    Proposed & 89\% & 10 s \\
  \end{tabular}
\end{frame}

\begin{frame}{Thank you}
  \centering
  Questions?
\end{frame}

\end{document}
"#;

const LETTER: &str = r#"\documentclass[11pt]{letter}
\usepackage[margin=2.5cm]{geometry}

\signature{Your Name}
\address{12 Example Street \\ Sample Town \\ 400001}

\begin{document}

\begin{letter}{Dr.\ Alex Morgan \\ Example Institute \\ 1 University Road \\ Sample City}

\opening{Dear Dr.\ Morgan,}

I am writing to apply for the research position advertised on your website. My training and experience match the requirements described in the advertisement.

Over the past two years I have worked on document preparation tools, which taught me to deliver reliable results to a deadline. I would welcome the chance to discuss how I can contribute to your group.

Thank you for your time and consideration.

\closing{Yours sincerely,}

\encl{Curriculum vitae}

\end{letter}

\end{document}
"#;

const CV: &str = r#"\documentclass[11pt,a4paper]{article}
\usepackage[margin=2cm]{geometry}
\usepackage[hidelinks]{hyperref}
\pagestyle{empty}
\setlength{\parindent}{0pt}

\newcommand{\entry}[3]{\textbf{#1} \hfill #2\\ \textit{#3}\par\smallskip}

\begin{document}

{\LARGE\bfseries Your Name}\\[2pt]
Sample Town \quad $\cdot$ \quad \href{mailto:you@example.com}{you@example.com} \quad $\cdot$ \quad +91 00000 00000
\bigskip

\section*{Experience}
\hrule\smallskip
\entry{Software Engineer, Example Corp}{2024 -- present}{Sample City}
\begin{itemize}\itemsep0pt
  \item Built a tool that reduced report preparation time by half.
  \item Led a team of three on a document automation project.
\end{itemize}

\entry{Intern, Sample Labs}{Summer 2023}{Sample City}
\begin{itemize}\itemsep0pt
  \item Wrote analysis scripts used in two published papers.
\end{itemize}

\section*{Education}
\hrule\smallskip
\entry{B.Tech.\ in Computer Engineering, Example University}{2020 -- 2024}{First class with distinction}

\section*{Skills}
\hrule\smallskip
\textbf{Languages:} Rust, Python, TypeScript \quad \textbf{Tools:} \LaTeX, Git, Linux

\end{document}
"#;

const UNICODE: &str = r#"% !TEX program = xelatex
\documentclass[11pt,a4paper]{article}
\usepackage[margin=2.5cm]{geometry}
\usepackage{fontspec}
\setmainfont{Latin Modern Roman}

\title{Unicode Text with XeLaTeX}
\author{Your Name}
\date{\today}

\begin{document}
\maketitle

\section{Accented letters}
Zürich, Málaga, São Paulo, Dvořák, Łódź, Ångström, Straße, façade, naïve, señor.

\section{Symbols}
Prices in € and £; fractions such as ½ and ¾; quotation marks “like this” and ‘this’; an em dash — and an en dash –; the degree sign 25°C; the multiplication sign 3 × 4 = 12.

\section{Ligatures and spacing}
The fi ligature in “office”, the ffl in “waffle”, and non-breaking spaces between a number and its unit: 5\,kg.

\end{document}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_has_its_root_file_and_a_unique_id() {
        let mut ids = std::collections::BTreeSet::new();
        for t in all() {
            assert!(ids.insert(t.id), "duplicate id {}", t.id);
            assert!(t.files.iter().any(|(p, _)| *p == t.root_file), "{} lacks its root file", t.id);
            assert!(t.files.iter().all(|(p, c)| !p.starts_with('/') && !p.contains("..") && !c.is_empty()));
        }
        assert!(all().len() >= 8);
    }

    #[test]
    fn the_tour_compiles_without_errors_when_tex_is_installed() {
        let Ok(install) = crate::tex::detect(None) else { return };
        if install.perl.is_none() {
            return;
        }
        let t = find("tour").unwrap();
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in t.files {
            crate::fsutil::write_text(dir.path(), path, text).unwrap();
        }
        let out = crate::compile::compile(&crate::compile::CompileRequest {
            install: &install,
            project_dir: dir.path(),
            root_file: t.root_file,
            engine: crate::tex::Engine::Pdflatex,
            build_dir: &dir.path().join(".build"),
            shell_escape: false,
            timeout: std::time::Duration::from_secs(300),
        })
        .unwrap();
        let errors: Vec<_> = out
            .diagnostics
            .iter()
            .filter(|d| d.severity == crate::compile::Severity::Error)
            .collect();
        assert!(out.success && out.pdf.is_some() && errors.is_empty(), "{errors:?}\n{}", out.console);
        assert!(!out.log.contains("Citation `") && !out.log.contains("Reference `"), "{}", out.log);
    }

    #[test]
    fn templates_are_found_by_id_in_any_case() {
        assert_eq!(find(" Paper ").unwrap().id, "paper");
        assert!(find("nonsense").is_none());
    }

    #[test]
    fn every_included_file_and_bibliography_exists_in_its_template() {
        for t in all() {
            let has = |name: &str| t.files.iter().any(|(p, _)| *p == name);
            for (_, text) in t.files {
                for line in text.lines() {
                    for key in ["\\input{", "\\include{"] {
                        if let Some((_, rest)) = line.split_once(key) {
                            let name = rest.split('}').next().unwrap();
                            assert!(has(&format!("{name}.tex")), "{}: {name}.tex is missing", t.id);
                        }
                    }
                    if let Some((_, rest)) = line.split_once("\\bibliography{") {
                        let name = rest.split('}').next().unwrap();
                        assert!(has(&format!("{name}.bib")), "{}: {name}.bib is missing", t.id);
                    }
                }
            }
        }
    }
}
