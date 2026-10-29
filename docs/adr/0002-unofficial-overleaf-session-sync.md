# Overleaf sync uses the logged-in web session, not Git or Dropbox

Overleaf's official Git, GitHub and Dropbox sync are premium-only, and the owner is on the free plan. Moss therefore signs in through an embedded Overleaf login window, keeps the session cookie in Windows Credential Manager, and uses Overleaf's private web endpoints (zip download for Pull, file upload and move for Push).

These endpoints are unsupported: Overleaf can change them without notice, and automating them may conflict with its Terms of Service. We accepted this because no free official route exists. The first milestone is a feasibility spike against the owner's real account, and the sync layer is isolated behind one interface so it can be swapped for Git if the owner upgrades.
