# pdf-toolkit

`pdf-toolkit` is a command-line utility written in pure Rust for converting
HTML files to PDF, adding attachments and merging PDFs plus some miscellaneous
functions.

It was motivated by the need to collect together code that can be used to
convert emails to PDF.


## Installation

To install `pdf-toolkit` from source, with [Rust](https://rust-lang.org) and
[git](https://git-scm.com) installed run the following command to put
`pdf-toolkit` (`pdf-toolkit.exe` on Windows) in `~/.cargo/bin`.

```
cargo install --git https://github.com/ggrothendieck/pdf-toolkit
```

## Commands

To get help run `pdf-toolkit --help` to list all commands or 
`pdf-toolkit merge --help`, etc.  to get help on particular commands.

In each case <INPUTS>... can be a space separated list of files, @filelist (where the
indicated file names one input file per line; blank lines and lines beginning with #
are ignored) or directories in which case all relevant files in the directory are used.

```
Command: html2pf
Description: Convert multiple HTML file(s), directories, or @file lists to PDF
Usage: pdf-toolkit html2pdf [OPTIONS] <INPUTS>...
Options:
   --outdir <OUTDIR> Target output directory for generated PDFs (defaults to '.')

Command: merge
Description: Merge multiple PDF documents or directories into a single file sequentially
Usage: pdf-toolkit merge [OPTIONS] <INPUTS>...
Options:
   -o, --output <OUTPUT> Output combined PDF file path

Command: pad-blank
Description: Append a blank page to the end of multiple PDF file(s), directories, or @file lists
Usage: pdf-toolkit pad-blank [OPTIONS] <INPUTS>...
Options:
   --outdir <OUTDIR> Target output directory (defaults to '.' for in-place modification)

Command: attach-to-pdf
Description: Embed one or more file attachments into multiple PDF file(s) (document or page level)
Usage: pdf-toolkit attach-to-pdf [OPTIONS] <INPUTS>...
Options:
   --attachments <ATTACHMENTS>... Files, directories, or @file lists to embed as attachments
   -l, --level <LEVEL> Attachment level: document or page (default: page)
   --no-sanitize Do not sanitize attachments.  Use only for trusted attachments.
   --outdir <OUTDIR> Target output directory (defaults to '.' for in-place modification)

Command: attach-to-html
Description: Embed one or more file attachments as downloadable data URIs into multiple HTML file(s)
Usage: pdf-toolkit attach-to-html [OPTIONS] <INPUTS>...
Options:
   --attachments <ATTACHMENTS>... Files, directories, or @file lists to embed as attachments
   --no-sanitize Do not sanitize attachments.  Use only for trusted attachments.
   --outdir <OUTDIR> Target output directory (defaults to '.' for in-place modification)
```

## Example

This example uses `mboxshell` to convert an mbox file to separate HTML files
into `outdir`.  Separately we extract the attachments and each is placed in
a directory in outdir having the same name as the associated file except 
without the extension.

Next we use `pdf-toolkit` to convert the HTML files to PDF, add the attachments
and merge them into a single PDF.

```
mboxshell export -f html --dirname seq-no --output outdir emails.mbox
mboxshell attachments --dirname seq-no --output outdir emails.mbox
cd outdir
pdf-toolkit html2pdf .
pdf-toolkit attach-to-pdf . --level page
pdf-toolkit merge . -o combined.pdf
```

## Attachments

Document vs. Page Level. The PDF format supports document level and page level
attachments.  Document level attachments apply to the entire document and are
typically rendered by PDF viewers in the left side panel.  Page level
attachments are typically represented by pin icons on the page they are
associated with.

For example, in the case of emails if the emails are to be merged into a single
PDF then page level attachments would be needed so that the attachments are
associated with the email they are attached to.  On the other hand if each
email is a separate PDF the either document level or page level attachments can
be used.

The advantage of document level attachments is that most PDF viewers support
them; however, they cannot be used to associate attachments to particular
pages.

PDF Viewers.  Some open source PDF viewers that support both page and document
level attachments are [Okular 26.11.70](https://github.com/KDE/okular] (cross
platform) and [SumatraPDF 3.7
prerelease](https://github.com/sumatrapdfreader/sumatrapdf) (Windows only).

Sanitizing. If the emails come from gmail, gmail will have scanned them for
viruses.  Attachments which are files on your system may have been scanned by
software that comes with your operating system or that you installed.  In
addition `pdf-toolkit` will reject certain attachments that are more likely to
contain code using the rust `infer` crate.  However, these measures are not
foolproof so simply do not add attachments if in doubt.  The `--no-sanitize`
flag can be used on the `attach-*` commands to disable sanitization by
`pdf-toolkit` in which case all attachments will be used, not just those that
infer passes.

## Internals

The following crates are used: Clap, anyhow, base64, infer, ironpress, lopdf,
pdf_oxide.

## License

MIT or Apache2.

