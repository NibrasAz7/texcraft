//! The curated command/environment catalog.
//!
//! One `static` table, sorted by [`CommandSpec::name`], authored from
//! CTAN package documentation. Names are byte-ordered: capitals sort
//! before lowercase, so e.g. `\SI` precedes `\addplot`. The sort test
//! at the bottom enforces the invariant the binary-search lookups in
//! `lib.rs` rely on.

/// The shape of a command's argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    /// Required `{...}` argument.
    Req,
    /// Optional `[...]` argument.
    Opt,
    /// Label or citation key `{...}`.
    Label,
    /// `key=value` option list.
    Key,
    /// File path argument (`.tex`, `.bib`, image...).
    File,
    /// TeX length/dimension argument.
    Length,
    /// Color name or specification.
    Color,
    /// Math-mode-only argument.
    Math,
}

/// One argument of a command or environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgSpec {
    pub kind: ArgKind,
    /// Placeholder name shown in the completion UI.
    pub name: &'static str,
}

/// A command or environment known to the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    /// Command or environment name, without the leading backslash.
    pub name: &'static str,
    /// Package providing it; `"latex2e"` for the kernel.
    pub package: &'static str,
    /// Signature, in argument order.
    pub args: &'static [ArgSpec],
    /// One-line description shown in completion.
    pub doc: &'static str,
    /// Superseded or discouraged; still completes, marked in the UI.
    pub deprecated: bool,
    /// `\begin{name}`…`\end{name}` environment rather than a command.
    pub is_env: bool,
}

const fn req(name: &'static str) -> ArgSpec {
    ArgSpec { kind: ArgKind::Req, name }
}
const fn opt(name: &'static str) -> ArgSpec {
    ArgSpec { kind: ArgKind::Opt, name }
}
const fn lbl(name: &'static str) -> ArgSpec {
    ArgSpec { kind: ArgKind::Label, name }
}
const fn key(name: &'static str) -> ArgSpec {
    ArgSpec { kind: ArgKind::Key, name }
}
const fn file(name: &'static str) -> ArgSpec {
    ArgSpec { kind: ArgKind::File, name }
}
const fn len(name: &'static str) -> ArgSpec {
    ArgSpec { kind: ArgKind::Length, name }
}
const fn col(name: &'static str) -> ArgSpec {
    ArgSpec { kind: ArgKind::Color, name }
}
const fn mth(name: &'static str) -> ArgSpec {
    ArgSpec { kind: ArgKind::Math, name }
}

const fn cmd(name: &'static str, package: &'static str, args: &'static [ArgSpec], doc: &'static str) -> CommandSpec {
    CommandSpec { name, package, args, doc, deprecated: false, is_env: false }
}
const fn env(name: &'static str, package: &'static str, args: &'static [ArgSpec], doc: &'static str) -> CommandSpec {
    CommandSpec { name, package, args, doc, deprecated: false, is_env: true }
}
const fn dcmd(name: &'static str, package: &'static str, args: &'static [ArgSpec], doc: &'static str) -> CommandSpec {
    CommandSpec { name, package, args, doc, deprecated: true, is_env: false }
}
const fn denv(name: &'static str, package: &'static str, args: &'static [ArgSpec], doc: &'static str) -> CommandSpec {
    CommandSpec { name, package, args, doc, deprecated: true, is_env: true }
}

/// The catalog table, sorted by `name` (enforced by tests).
pub(crate) static CATALOG: &[CommandSpec] = &[
    cmd("DeclareMathOperator", "amsmath", &[req("name"), req("definition")], "Defines a new math operator like \\sin or \\lim."),
    cmd("Huge", "latex2e", &[], "Largest font-size switch."),
    cmd("LARGE", "latex2e", &[], "Font-size switch between \\Large and \\huge."),
    cmd("Large", "latex2e", &[], "Font-size switch, two steps above \\normalsize."),
    cmd("RequirePackage", "latex2e", &[opt("options"), req("package")], "Loads a package from inside a class or package file."),
    cmd("SI", "siunitx", &[opt("options"), req("value"), req("unit")], "Quantity with unit (siunitx v2 name; \\qty is the v3 spelling)."),
    env("Verbatim", "fancyvrb", &[opt("options")], "Verbatim with key=value options from fancyvrb."),
    env("Vmatrix", "amsmath", &[], "Math matrix with double-bar delimiters."),
    env("abstract", "latex2e", &[], "Abstract block for article/report-style front matter."),
    cmd("addbibresource", "biblatex", &[file("bibfile")], "Registers a .bib file for biblatex (biber/BibTeX backend)."),
    cmd("addplot", "pgfplots", &[opt("options")], "Plots coordinates, a table or a function inside an axis environment."),
    cmd("address", "letter", &[req("sender-address")], "Sender's address in a letter; KOMA scrlttr2 uses \\setkomavar{fromaddress} instead."),
    cmd("addtocounter", "latex2e", &[req("counter"), req("value")], "Adds a value to a counter."),
    cmd("addtolength", "latex2e", &[len("register"), len("amount")], "Adds to a length register."),
    cmd("alert", "beamer", &[opt("overlay"), req("text")], "Highlights text on the given overlay slides."),
    env("alertblock", "beamer", &[req("title")], "Alert (red) block in a beamer frame."),
    env("align", "amsmath", &[], "Multi-line equation alignment; align* is unnumbered, \\nonumber suppresses one row."),
    env("alignat", "amsmath", &[req("column-pairs")], "align variant with explicit column count and no inter-column spacing."),
    env("aligned", "amsmath", &[opt("pos")], "Aligned math block for use inside another equation."),
    cmd("appendix", "latex2e", &[], "Switches sectioning to appendices (A, B, ...)."),
    cmd("arPar", "arabdoc", &[req("text")], "WarraqCraft bundled package: typesets a right-to-left Arabic paragraph."),
    cmd("arText", "arabdoc", &[req("text")], "WarraqCraft bundled package: inline Arabic text, right-to-left."),
    cmd("arabicsection", "arabdoc", &[opt("short"), req("title")], "WarraqCraft bundled package: Arabic-titled section heading."),
    env("array", "latex2e", &[req("cols")], "Math-mode table environment; column spec like tabular."),
    cmd("author", "latex2e", &[req("names")], "Sets the author list printed by \\maketitle."),
    cmd("autocite", "biblatex", &[opt("prenote"), lbl("key")], "Citation in the style's smart format (parentheses or footnote)."),
    cmd("autoref", "hyperref", &[lbl("label")], "Reference prefixed with the object type, e.g. 'Figure 1'."),
    env("axis", "pgfplots", &[opt("options")], "Axis environment holding \\addplot commands."),
    cmd("backmatter", "latex2e", &[], "Book-class switch for back matter (bibliography, index)."),
    cmd("begin", "latex2e", &[req("environment")], "Opens an environment; every \\begin needs a matching \\end."),
    cmd("bibliography", "latex2e", &[file("bibfile")], "Prints the BibTeX bibliography built from a .bib file."),
    cmd("bibliographystyle", "latex2e", &[req("style")], "Selects a BibTeX .bst style: plain, abbrv, alpha, unsrt."),
    cmd("biggl", "latex2e", &[req("delimiter")], "Left delimiter one size above \\bigl."),
    cmd("biggr", "latex2e", &[req("delimiter")], "Right delimiter one size above \\bigr."),
    cmd("bigl", "latex2e", &[req("delimiter")], "Left fixed-size math delimiter, e.g. \\bigl( ... \\bigr)."),
    cmd("bigr", "latex2e", &[req("delimiter")], "Right fixed-size math delimiter."),
    cmd("binom", "amsmath", &[mth("top"), mth("bottom")], "Binomial coefficient."),
    env("block", "beamer", &[req("title")], "Standard titled block in a beamer frame."),
    env("bmatrix", "amsmath", &[], "Math matrix with square-bracket delimiters."),
    cmd("bmod", "amsmath", &[], "Binary mod operator: a \\bmod b."),
    cmd("bottomrule", "booktabs", &[opt("width")], "Heavy rule at the foot of a booktabs table."),
    cmd("boxed", "amsmath", &[mth("content")], "Draws a box around math content."),
    cmd("caption", "latex2e", &[opt("short"), req("text")], "Caption inside a figure/table float; optional short form for the lists."),
    cmd("captionsetup", "caption", &[key("options")], "Sets caption package options: labelfont, position, justification."),
    env("cases", "amsmath", &[], "Brace-delimited case distinctions in math."),
    cmd("cdots", "latex2e", &[], "Centered ellipsis dots in math."),
    env("center", "latex2e", &[], "Centers each line of the environment."),
    cmd("chapter", "latex2e", &[opt("short"), req("title")], "Chapter heading (book/report); \\chapter* is unnumbered."),
    cmd("cite", "latex2e", &[opt("note"), lbl("key")], "Citation; biblatex and natbib extend it with richer variants."),
    cmd("citealp", "natbib", &[opt("note"), lbl("key")], "\\citep without the parentheses."),
    cmd("citealt", "natbib", &[opt("note"), lbl("key")], "\\citet without the parentheses."),
    cmd("citeauthor", "natbib", &[lbl("key")], "Prints only the author names of the citation."),
    cmd("citep", "natbib", &[opt("note"), lbl("key")], "Parenthetical author-year citation: (Knuth, 1984)."),
    cmd("citet", "natbib", &[opt("note"), lbl("key")], "Textual citation: Knuth (1984)."),
    cmd("citeyear", "natbib", &[lbl("key")], "Prints only the year of the citation."),
    cmd("clearpage", "latex2e", &[], "Ends the page and flushes all pending floats."),
    cmd("closing", "letter", &[req("text")], "Closing formula of a letter, e.g. 'Yours sincerely,'."),
    cmd("cmidrule", "booktabs", &[opt("trim"), req("range")], "Rule spanning a column range, e.g. \\cmidrule(r){1-2}."),
    cmd("color", "xcolor", &[opt("model"), col("color")], "Switches text color; optional model like rgb, cmyk, HTML."),
    cmd("colorbox", "xcolor", &[col("color"), req("text")], "Text on a colored background box."),
    env("column", "beamer", &[req("width")], "One column inside a beamer columns environment."),
    env("columns", "beamer", &[opt("options")], "Multi-column layout inside a beamer frame."),
    cmd("coordinate", "tikz", &[opt("options"), req("name")], "Names a point on the path; 'at (x,y)' follows."),
    cmd("date", "latex2e", &[req("date")], "Sets the date for \\maketitle; an empty {} suppresses it."),
    cmd("dbinom", "amsmath", &[mth("top"), mth("bottom")], "Display-style binomial coefficient."),
    cmd("definecolor", "xcolor", &[req("name"), req("model"), req("spec")], "Defines a named color in a model: rgb, cmyk, gray, HTML."),
    env("description", "latex2e", &[opt("options")], "Labeled list; \\item[term] sets the bold term. Options come from enumitem."),
    cmd("dfrac", "amsmath", &[mth("numerator"), mth("denominator")], "Display-size fraction."),
    env("document", "latex2e", &[], "Document body environment; exactly one per file."),
    cmd(
        "documentclass",
        "latex2e",
        &[opt("options"), req("class")],
        "First line of a .tex file; picks the class: article, book, beamer, scrartcl...",
    ),
    cmd("dots", "latex2e", &[], "Context-aware ellipsis in math or text."),
    cmd("draw", "tikz", &[opt("options")], "Strokes a path; color, width and arrows go in the options."),
    cmd("emph", "latex2e", &[req("text")], "Emphasis; toggles italic/upright depending on context."),
    cmd("end", "latex2e", &[req("environment")], "Closes the environment opened by \\begin."),
    cmd("enquote", "csquotes", &[req("text")], "Language-aware quotation marks."),
    env("enumerate", "latex2e", &[opt("options")], "Numbered list; bracket options like label= come from enumitem."),
    denv("eqnarray", "latex2e", &[], "Legacy math alignment with poor spacing; use amsmath align instead."),
    cmd("eqref", "amsmath", &[lbl("label")], "Equation reference printed as (1)."),
    env("equation", "latex2e", &[], "Single numbered equation; equation* is unnumbered."),
    env("exampleblock", "beamer", &[req("title")], "Example (green) block in a beamer frame."),
    cmd("fancyfoot", "fancyhdr", &[opt("places"), req("content")], "Sets footer fields L/C/R and O/E, e.g. \\fancyfoot[LE,RO]{\\thepage}."),
    cmd("fancyhead", "fancyhdr", &[opt("places"), req("content")], "Sets header fields L/C/R and O/E."),
    dcmd("fancyheadings", "fancyhdr", &[], "Legacy page style; prefer the fancy style with \\fancyhf."),
    cmd("fancyhf", "fancyhdr", &[opt("places"), req("content")], "Sets or clears header/footer fields; empty content clears them."),
    cmd("fcolorbox", "xcolor", &[col("frame"), col("background"), req("text")], "Boxed text with frame and background colors."),
    env("figure", "latex2e", &[opt("placement")], "Floating figure; placement letters h, t, b, p."),
    cmd("fill", "tikz", &[opt("options")], "Fills a path; coordinates or shapes follow."),
    env("flushleft", "latex2e", &[], "Left-aligned, ragged-right text."),
    env("flushright", "latex2e", &[], "Right-aligned, ragged-left text."),
    cmd("footcite", "biblatex", &[opt("prenote"), lbl("key")], "Footnote citation."),
    cmd("footnote", "latex2e", &[opt("number"), req("text")], "Footnote at the bottom of the page."),
    cmd("footnotemark", "latex2e", &[opt("number")], "Footnote marker only; the text goes in \\footnotetext."),
    cmd("footnotesize", "latex2e", &[], "Font-size switch above \\scriptsize."),
    cmd("footnotetext", "latex2e", &[opt("number"), req("text")], "Footnote text for a mark set elsewhere."),
    env("forest", "forest", &[opt("options")], "Tree-drawing environment (forest)."),
    cmd("frac", "latex2e", &[mth("numerator"), mth("denominator")], "Fraction in math mode."),
    env("frame", "beamer", &[opt("options")], "One slide; options include fragile and allowframebreaks."),
    cmd("framesubtitle", "beamer", &[req("subtitle")], "Subtitle line of a beamer frame."),
    cmd("frametitle", "beamer", &[req("title")], "Title line of a beamer frame."),
    cmd("frontmatter", "latex2e", &[], "Book switch: roman page numbers and unnumbered chapters."),
    cmd("fullcite", "biblatex", &[lbl("key")], "Full bibliography entry used as a citation."),
    env("gather", "amsmath", &[], "Centered multi-line equations, each numbered."),
    env("gathered", "amsmath", &[opt("pos")], "Gathered block for use inside another equation."),
    cmd("geometry", "geometry", &[key("options")], "Page layout keys: margin, top/bottom/left/right, paper=a4paper or letterpaper."),
    cmd("hfill", "latex2e", &[], "Horizontal spring space pushing content apart."),
    cmd("hline", "latex2e", &[], "Full-width horizontal rule in tabular/array."),
    cmd("href", "hyperref", &[req("url"), req("text")], "Hyperlinked text pointing at a URL."),
    cmd("hspace", "latex2e", &[len("amount")], "Horizontal space; \\hspace* survives at line breaks."),
    cmd("huge", "latex2e", &[], "Font-size switch just below \\Huge."),
    cmd("hyperref", "hyperref", &[opt("label"), req("text")], "Text hyperlinked to an internal label."),
    cmd("hypersetup", "hyperref", &[key("options")], "hyperref options: colorlinks, linkcolor, urlcolor, pdftitle..."),
    cmd("include", "latex2e", &[file("file")], "Includes a .tex file with a page break; honors \\includeonly."),
    cmd(
        "includegraphics",
        "graphicx",
        &[key("options"), file("file")],
        "Inserts an image; keys: width, height, scale, angle, keepaspectratio, page, trim, clip, draft.",
    ),
    cmd("includeonly", "latex2e", &[file("files")], "Comma list of files allowed through \\include."),
    cmd("infty", "latex2e", &[], "Infinity symbol in math."),
    cmd("input", "latex2e", &[file("file")], "Inputs a .tex file inline, no page break."),
    cmd("int", "latex2e", &[], "Integral sign in math."),
    cmd("intertext", "amsmath", &[req("text")], "A line of text inside an align block, keeping alignment."),
    cmd("item", "latex2e", &[opt("label")], "List item; optional bracket label overrides the marker."),
    env("itemize", "latex2e", &[opt("options")], "Bulleted list; bracket options like label= come from enumitem."),
    cmd("label", "latex2e", &[lbl("name")], "Assigns a label for \\ref, \\eqref or \\autoref."),
    cmd("large", "latex2e", &[], "Font-size switch one step above \\normalsize."),
    cmd("ldots", "latex2e", &[], "Baseline ellipsis dots in math."),
    cmd("left", "latex2e", &[req("delimiter")], "Auto-sized left math delimiter; pair with \\right."),
    env("letter", "letter", &[req("recipient")], "Letter environment; KOMA's scrlttr2 class uses the same commands."),
    cmd("lim", "latex2e", &[], "Limit operator in math."),
    cmd("linebreak", "latex2e", &[opt("priority")], "Encourages a line break; optional urgency 0-4."),
    cmd("listoffigures", "latex2e", &[], "Prints the list of figures."),
    cmd("listoftables", "latex2e", &[], "Prints the list of tables."),
    env("longtable", "longtable", &[req("cols")], "Multi-page table environment."),
    cmd("lstinline", "listings", &[opt("options")], "Inline code delimited by any character: \\lstinline|code|."),
    cmd("lstinputlisting", "listings", &[opt("options"), file("file")], "Typesets an external source file as a listing."),
    env("lstlisting", "listings", &[opt("options")], "Code listing environment; [language=C] etc."),
    cmd("lstset", "listings", &[key("options")], "Global listings options: language, basicstyle, numbers."),
    cmd("mainmatter", "latex2e", &[], "Book switch: arabic page numbers and numbered chapters."),
    cmd("maketitle", "latex2e", &[], "Prints the title block built by \\title, \\author, \\date."),
    cmd("marginpar", "latex2e", &[opt("left"), req("text")], "Marginal note; optional variant for two-sided printing."),
    cmd("mathbb", "amssymb", &[mth("text")], "Blackboard bold, e.g. \\mathbb{R}; needs amssymb."),
    cmd("mathbf", "latex2e", &[mth("text")], "Bold upright math letters."),
    cmd("mathcal", "latex2e", &[mth("text")], "Calligraphic capitals in math."),
    cmd("mathfrak", "amssymb", &[mth("text")], "Fraktur letters; needs amssymb."),
    cmd("mathit", "latex2e", &[mth("text")], "Italic math letters."),
    cmd("mathrm", "latex2e", &[mth("text")], "Upright math letters."),
    cmd("mathsf", "latex2e", &[mth("text")], "Sans-serif math letters."),
    cmd("mathtt", "latex2e", &[mth("text")], "Typewriter math letters."),
    env("matrix", "amsmath", &[], "Bare math matrix without delimiters."),
    env("mdframed", "mdframed", &[opt("options")], "Framed, page-breakable environment (mdframed)."),
    cmd("midrule", "booktabs", &[opt("width")], "Rule between a booktabs header and body."),
    env("minipage", "latex2e", &[opt("pos"), len("width")], "Box typeset as a small page of the given width."),
    cmd("mod", "amsmath", &[req("modulus")], "'mod' operator without parentheses."),
    cmd("multicolumn", "latex2e", &[req("n"), req("cols"), req("text")], "Tabular cell spanning n columns."),
    cmd("multirow", "multirow", &[req("rows"), req("width"), req("text")], "Cell spanning several rows; needs the multirow package."),
    env("multline", "amsmath", &[], "Long equation broken over lines: first left, last right."),
    cmd("nabla", "latex2e", &[], "Nabla (del) operator in math."),
    cmd("newcommand", "latex2e", &[req("command"), opt("nargs"), req("definition")], "Defines a new command; fails if the name is taken."),
    cmd("newcounter", "latex2e", &[req("name"), opt("within")], "Declares a counter, optionally reset within another."),
    cmd("newenvironment", "latex2e", &[req("name"), opt("nargs"), req("begin-code"), req("end-code")], "Defines a new environment."),
    cmd("newfontfamily", "fontspec", &[req("command"), opt("features"), req("font")], "Binds a font family to a new command (XeTeX/LuaTeX)."),
    cmd("newlength", "latex2e", &[req("name")], "Declares a length register."),
    cmd("newline", "latex2e", &[], "Forced line break inside a paragraph."),
    cmd("newpage", "latex2e", &[], "Ends the current page."),
    cmd(
        "newtheorem",
        "latex2e",
        &[req("name"), opt("numbered-like"), req("heading"), opt("numbered-within")],
        "Declares a theorem-like environment.",
    ),
    cmd("nocite", "latex2e", &[req("keys")], "Adds entries to the bibliography without citing; {*} includes all."),
    cmd("node", "tikz", &[opt("options"), req("text")], "Places text on a path; 'at (x,y)' sets the position."),
    cmd("normalsize", "latex2e", &[], "The document's base font size."),
    cmd("num", "siunitx", &[opt("options"), req("number")], "Formats a number with locale-aware separators."),
    cmd("only", "beamer", &[req("overlay"), req("text")], "Shows content only on the given overlay slides."),
    cmd("onslide", "beamer", &[req("overlay")], "Overlay specifier; the following content appears on given slides."),
    cmd("opening", "letter", &[req("text")], "Salutation of a letter, e.g. 'Dear ...,'."),
    cmd("operatorname", "amsmath", &[req("name")], "Typesets an operator name upright inside math."),
    cmd("overset", "amsmath", &[mth("above"), mth("base")], "Puts a symbol or label above a base."),
    cmd("pagebreak", "latex2e", &[opt("priority")], "Encourages a page break; optional urgency 0-4."),
    cmd("pageref", "latex2e", &[lbl("label")], "Page number of a label."),
    cmd("pagestyle", "latex2e", &[req("style")], "Page style: empty, plain, headings, myheadings or fancy."),
    cmd("paragraph", "latex2e", &[opt("short"), req("title")], "Paragraph-level heading; \\paragraph* is unnumbered."),
    cmd("parbox", "latex2e", &[opt("pos"), len("width"), req("text")], "Paragraph-mode box of a fixed width."),
    cmd("parencite", "biblatex", &[opt("prenote"), lbl("key")], "Parenthetical citation: (Author 2020)."),
    cmd("part", "latex2e", &[opt("short"), req("title")], "Part-level heading; \\part* is unnumbered."),
    cmd("partial", "latex2e", &[], "Partial derivative symbol in math."),
    cmd("path", "tikz", &[opt("options")], "Generic path command; the basis of \\draw, \\fill, \\node."),
    cmd("pause", "beamer", &[opt("number")], "Pause between overlay steps in a frame."),
    cmd("pdfstringdefDisableCommands", "hyperref", &[req("code")], "Substitutes commands inside PDF bookmarks and strings."),
    env("pmatrix", "amsmath", &[], "Math matrix with parenthesis delimiters."),
    cmd("pmod", "amsmath", &[req("modulus")], "Parenthesized mod: (mod m)."),
    cmd("pod", "amsmath", &[req("modulus")], "Parenthesized mod without the 'mod' text."),
    cmd("printbibliography", "biblatex", &[opt("options")], "Prints the reference list; options filter by type, heading, etc."),
    cmd("prod", "latex2e", &[], "Product operator in math."),
    cmd("protect", "latex2e", &[], "Protects a fragile command inside a moving argument."),
    cmd("providecommand", "latex2e", &[req("command"), opt("nargs"), req("definition")], "Defines a command only if it does not already exist."),
    cmd("qty", "siunitx", &[opt("options"), req("value"), req("unit")], "Quantity with unit (siunitx v3; \\SI is the v2 name)."),
    env("quotation", "latex2e", &[], "Long quotation with indented paragraphs."),
    env("quote", "latex2e", &[], "Short quotation, indented as a block."),
    cmd("raisebox", "latex2e", &[len("lift"), opt("height"), opt("depth"), req("text")], "Raises text by a length; optional apparent height/depth."),
    cmd("ref", "latex2e", &[lbl("label")], "Number of a label."),
    cmd("reflectbox", "graphicx", &[req("text")], "Mirrors text horizontally."),
    cmd("refstepcounter", "latex2e", &[req("counter")], "Steps a counter and makes it the target of \\label."),
    cmd("renewcommand", "latex2e", &[req("command"), opt("nargs"), req("definition")], "Redefines an existing command."),
    cmd("resizebox", "graphicx", &[len("width"), len("height"), req("text")], "Scales content to a given size; ! keeps the aspect ratio."),
    cmd("right", "latex2e", &[req("delimiter")], "Auto-sized right math delimiter; pair with \\left."),
    cmd("rotatebox", "graphicx", &[opt("options"), req("angle"), req("text")], "Rotates content by an angle in degrees."),
    cmd("rule", "latex2e", &[opt("raise"), len("width"), len("height")], "Solid rectangle, e.g. \\rule{\\linewidth}{0.4pt}."),
    cmd("scalebox", "graphicx", &[req("hscale"), opt("vscale"), req("text")], "Scales content horizontally, and vertically if given."),
    cmd("scriptsize", "latex2e", &[], "Font-size switch above \\tiny."),
    cmd("section", "latex2e", &[opt("short"), req("title")], "Section heading; \\section* is unnumbered."),
    cmd("selectlanguage", "babel", &[req("language")], "Switches hyphenation and caption language (babel)."),
    cmd("setcounter", "latex2e", &[req("counter"), req("value")], "Sets a counter's value."),
    cmd("setlength", "latex2e", &[len("register"), len("value")], "Assigns a value to a length register, e.g. \\parindent."),
    cmd("setlist", "enumitem", &[opt("level"), key("options")], "Sets enumitem defaults, e.g. \\setlist[itemize]{label=--}."),
    cmd("setmainfont", "fontspec", &[opt("features"), req("font")], "Main serif font for XeTeX/LuaTeX documents."),
    cmd("setmainlanguage", "polyglossia", &[opt("options"), req("language")], "Main document language, including RTL languages like Arabic."),
    cmd("setmonofont", "fontspec", &[opt("features"), req("font")], "Monospace font for XeTeX/LuaTeX documents."),
    cmd("setotherlanguage", "polyglossia", &[opt("options"), req("language")], "Secondary language, e.g. \\setotherlanguage{arabic}."),
    cmd("setsansfont", "fontspec", &[opt("features"), req("font")], "Sans-serif font for XeTeX/LuaTeX documents."),
    cmd("setstretch", "setspace", &[req("factor")], "Line-spacing factor (setspace)."),
    cmd("si", "siunitx", &[opt("options"), req("unit")], "Standalone unit (siunitx; \\unit in v3)."),
    cmd("signature", "letter", &[req("name")], "Name printed in the signature of a letter."),
    cmd("small", "latex2e", &[], "Font-size switch below \\normalsize."),
    env("smallmatrix", "amsmath", &[], "Inline-sized math matrix."),
    cmd("smartcite", "biblatex", &[opt("prenote"), lbl("key")], "Context-aware citation (footnote inline, parenthesis in notes)."),
    env("spacing", "setspace", &[req("factor")], "Applies a line-spacing factor to the environment."),
    env("split", "amsmath", &[], "Breaks a single equation over lines inside equation."),
    cmd("sqrt", "latex2e", &[opt("root"), mth("radicand")], "Square root; optional bracket gives the nth root."),
    cmd("stepcounter", "latex2e", &[req("counter")], "Increments a counter and resets its children."),
    cmd("subcaption", "subcaption", &[opt("short"), req("text")], "Caption for a sub-figure or sub-table inside the float."),
    env("subequations", "amsmath", &[], "Numbers contained equations 1a, 1b, ..."),
    cmd("subparagraph", "latex2e", &[opt("short"), req("title")], "Lowest heading level; \\subparagraph* is unnumbered."),
    cmd("subsection", "latex2e", &[opt("short"), req("title")], "Subsection heading; \\subsection* is unnumbered."),
    cmd("subsubsection", "latex2e", &[opt("short"), req("title")], "Sub-subsection heading; \\subsubsection* is unnumbered."),
    cmd("sum", "latex2e", &[], "Summation operator in math."),
    cmd("supercite", "biblatex", &[lbl("key")], "Superscript citation (numeric styles)."),
    env("table", "latex2e", &[opt("placement")], "Floating table; placement letters h, t, b, p."),
    cmd("tableofcontents", "latex2e", &[], "Prints the table of contents."),
    env("tabular", "latex2e", &[opt("pos"), req("cols")], "Text table; column spec: l c r, |, @{...}, p{width}."),
    env("tabularx", "tabularx", &[len("width"), req("cols")], "Fixed-width table whose X columns stretch."),
    cmd("tag", "amsmath", &[req("tag")], "Custom equation tag, e.g. \\tag{*}."),
    cmd("tbinom", "amsmath", &[mth("top"), mth("bottom")], "Text-style binomial coefficient."),
    cmd("text", "amsmath", &[req("text")], "Upright text inside math mode."),
    cmd("textbf", "latex2e", &[req("text")], "Bold text."),
    cmd("textcite", "biblatex", &[opt("prenote"), lbl("key")], "Textual citation: Author (2020)."),
    cmd("textcolor", "xcolor", &[opt("model"), col("color"), req("text")], "Colored text without changing the current color."),
    cmd("textit", "latex2e", &[req("text")], "Italic text."),
    cmd("textrm", "latex2e", &[req("text")], "Roman (serif) text."),
    cmd("textsc", "latex2e", &[req("text")], "Small-caps text."),
    cmd("textsf", "latex2e", &[req("text")], "Sans-serif text."),
    cmd("texttt", "latex2e", &[req("text")], "Typewriter (monospace) text."),
    cmd("tfrac", "amsmath", &[mth("numerator"), mth("denominator")], "Text-size fraction."),
    env("tikzpicture", "tikz", &[opt("options")], "TikZ drawing canvas."),
    cmd("tiny", "latex2e", &[], "Smallest font-size switch."),
    cmd("title", "latex2e", &[req("title")], "Sets the title printed by \\maketitle."),
    cmd(
        "titleformat",
        "titlesec",
        &[req("command"), opt("shape"), req("format"), req("label"), req("sep"), req("before-code"), opt("after-code")],
        "Restyles a sectioning command's title.",
    ),
    cmd("titlespacing", "titlesec", &[req("command"), len("left"), len("before"), len("after")], "Restyles spacing around a sectioning title."),
    cmd("today", "latex2e", &[], "Current date in the document's format."),
    cmd("toprule", "booktabs", &[opt("width")], "Heavy rule at the head of a booktabs table."),
    cmd("uncover", "beamer", &[req("overlay"), req("text")], "Reveals content on given slides, dimmed before."),
    cmd("underline", "latex2e", &[req("text")], "Underlined text."),
    cmd("underset", "amsmath", &[mth("below"), mth("base")], "Puts a symbol or label below a base."),
    cmd("unit", "siunitx", &[opt("options"), req("unit")], "Standalone unit (siunitx v3; \\si is the v2 name)."),
    cmd("url", "hyperref", &[req("url")], "Bare URL that may break over lines."),
    cmd("urlstyle", "hyperref", &[req("style")], "Font for URLs: same, tt, rm or sf."),
    cmd("useasboundingbox", "tikz", &[opt("options")], "Uses the following path as the picture's bounding box."),
    cmd("usepackage", "latex2e", &[opt("options"), req("package")], "Loads a package in the preamble."),
    cmd("value", "latex2e", &[req("counter")], "Expands to a counter's numeric value for arithmetic."),
    cmd("verb", "latex2e", &[], "Inline verbatim delimited by any character: \\verb|code|."),
    env("verbatim", "latex2e", &[], "Verbatim environment printing input literally."),
    env("verse", "latex2e", &[], "Verse environment for poetry."),
    cmd("vfill", "latex2e", &[], "Vertical spring space pushing content apart."),
    cmd("visible", "beamer", &[req("overlay"), req("text")], "Like \\uncover but keeps the space instead of dimming."),
    env("vmatrix", "amsmath", &[], "Math matrix with single-bar delimiters (determinant)."),
    cmd("vspace", "latex2e", &[len("amount")], "Vertical space; \\vspace* survives at page breaks."),
    cmd("xleftarrow", "amsmath", &[opt("below"), mth("above")], "Extensible left arrow with labels."),
    cmd("xrightarrow", "amsmath", &[opt("below"), mth("above")], "Extensible right arrow with labels."),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_large_enough() {
        assert!(CATALOG.len() > 140, "catalog has {} entries", CATALOG.len());
    }

    #[test]
    fn catalog_is_sorted_and_unique() {
        for w in CATALOG.windows(2) {
            assert!(w[0].name < w[1].name, "out of order or duplicate: {:?} then {:?}", w[0].name, w[1].name);
        }
    }

    #[test]
    fn every_env_is_reachable_via_environments() {
        for c in CATALOG.iter().filter(|c| c.is_env) {
            assert!(crate::environments().any(|e| core::ptr::eq(e, c)), "env {} not reachable", c.name);
        }
    }

    #[test]
    fn find_prefix_finds_subsection_family() {
        let hits: Vec<&str> = crate::find_prefix("sub").map(|c| c.name).collect();
        assert!(hits.contains(&"subsection"), "{hits:?}");
        assert!(hits.contains(&"subsubsection"), "{hits:?}");
        assert!(hits.contains(&"subparagraph"), "{hits:?}");
    }

    #[test]
    fn packages_include_core_packages() {
        let pkgs: Vec<&str> = crate::packages().collect();
        for want in ["amsmath", "hyperref", "biblatex"] {
            assert!(pkgs.contains(&want), "missing {want} in {pkgs:?}");
        }
        // And the list is sorted + deduplicated by construction.
        let mut sorted = pkgs.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(pkgs, sorted);
    }

    #[test]
    fn find_exact_hits_and_misses() {
        assert_eq!(crate::find_exact("section").map(|c| c.package), Some("latex2e"));
        assert!(crate::find_exact("no-such-command").is_none());
    }

    #[test]
    fn every_entry_has_a_doc_line() {
        for c in CATALOG {
            assert!(!c.doc.trim().is_empty(), "{} has an empty doc", c.name);
        }
    }
}
