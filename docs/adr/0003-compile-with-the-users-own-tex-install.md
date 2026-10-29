# Compile with `latexmk` on the user's own TeX installation

The earlier Moss builds tried a browser WASM compiler (SwiftLaTeX) and Tectonic. Both fell short of full LaTeX compatibility. Moss now drives `latexmk` against a real MiKTeX or TeX Live install on the user's machine, which gives every engine and package Overleaf offers.

We do not bundle TeX (a full distribution is several GB). Instead the app detects an existing install, verifies it with a test compile, and offers a one-click MiKTeX plus Perl install when none is found. `latexmk` needs Perl on Windows, so Perl is part of that setup.
