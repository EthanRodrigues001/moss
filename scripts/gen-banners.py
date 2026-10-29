import re,base64,pathlib
root=pathlib.Path(r"C:\Users\Ethan Rodrigues\Documents\Ethan\Projects\Moss")
logo=(root/"assets/brand/moss-logo.svg").read_text(encoding="utf-8")
defs=re.search(r"<defs>(.*?)</defs>",logo,re.S).group(1)
defs=re.sub(r'<clipPath id="moss-tile">.*?</clipPath>',"",defs,flags=re.S)
art=re.search(r'(<g transform="rotate\(-14 256 256\)">.*?</g>\s*)</g>\s*<!-- tile rim',logo,re.S).group(1)
b64=base64.b64encode((root/"assets/brand/moss-logo.svg").read_bytes()).decode()
FONT="font-family=\"'Segoe UI Variable Display','Segoe UI',system-ui,Arial,sans-serif\""
def svg(W,H,tx,ty,s,extra="",rot=0):
    return f'''<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 {W} {H}" width="{W}" height="{H}">
<defs>{defs}<clipPath id="bn"><rect width="{W}" height="{H}"/></clipPath>
<radialGradient id="vig" cx="0.2" cy="0.5" r="0.9"><stop offset="0" stop-color="#000" stop-opacity=".0"/><stop offset="1" stop-color="#000" stop-opacity=".35"/></radialGradient></defs>
<rect width="{W}" height="{H}" fill="url(#moss-base)"/>
<rect width="{W}" height="{H}" fill="url(#vig)"/>
<g clip-path="url(#bn)"><g transform="translate({tx} {ty}) scale({s}) rotate({rot} 256 256)">{art}</g></g>
{extra}</svg>'''
img=lambda x,y,w:f'<image x="{x}" y="{y}" width="{w}" height="{w}" xlink:href="data:image/svg+xml;base64,{b64}"/>'
# in-app banner: artwork only (logo and title are live HTML on top)
(root/"ui/public/setup-banner.svg").write_text(svg(1200,300,640,-40,0.95),encoding="utf-8")
# NSIS sidebar 164x314 @2x
side=svg(328,628,70,370,0.8)
side=side.replace("</svg>",img(24,28,120)+f'<text x="26" y="196" {FONT} font-size="46" font-weight="700" fill="#fff" letter-spacing="-1">Moss</text><text x="28" y="226" {FONT} font-size="18" fill="#b8c4e6">LaTeX on your PC</text></svg>')
pathlib.Path("side.svg").write_text(side,encoding="utf-8")
# NSIS header 150x57 @4x
head=svg(600,228,210,-30,0.7)
head=head.replace("</svg>",img(24,44,140)+"</svg>")
pathlib.Path("head.svg").write_text(head,encoding="utf-8")
