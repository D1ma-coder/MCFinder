#!/usr/bin/env python3
"""Build the original MCFinder pixel wordmark as a self-contained animated SVG."""
from pathlib import Path

FONT = {
    'M': ['10001','11011','10101','10101','10001','10001','10001'],
    'C': ['01111','11000','10000','10000','10000','11000','01111'],
    'F': ['11111','10000','10000','11110','10000','10000','10000'],
    'i': ['010','000','110','010','010','010','111'],
    'n': ['00000','00000','11110','10001','10001','10001','10001'],
    'd': ['00001','00001','01111','10001','10001','10001','01111'],
    'e': ['00000','00000','01110','10001','11111','10000','01111'],
    'r': ['0000','0000','1011','1100','1000','1000','1000'],
}

def outline(rows):
    edges = set()
    for y, row in enumerate(rows):
        for x, bit in enumerate(row):
            if bit != '1': continue
            vertices = [(x,y),(x+1,y),(x+1,y+1),(x,y+1)]
            for a,b in zip(vertices,vertices[1:]+vertices[:1]):
                if (b,a) in edges: edges.remove((b,a))
                else: edges.add((a,b))
    paths = []
    while edges:
        a,b = min(edges); edges.remove((a,b)); start=a
        commands=[f'M{a[0]*18} {a[1]*18}',f'L{b[0]*18} {b[1]*18}']
        while b != start:
            # All glyph boundaries have one outgoing edge at each vertex.
            edge=next(edge for edge in sorted(edges) if edge[0]==b)
            edges.remove(edge); b=edge[1]; commands.append(f'L{b[0]*18} {b[1]*18}')
        paths.append(' '.join(commands)+'Z')
    return ' '.join(paths)

pieces=[]
x=0
for letter in 'MCFinder':
    pieces.append((x,outline(FONT[letter])))
    x+=(len(FONT[letter][0])+1)*18
width=x-18
start=(1000-width-10)/2
paths='\n'.join(f'<path d="{path}" transform="translate({offset} 0)"/>' for offset,path in pieces)
svg=f'''<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="260" viewBox="0 0 1000 260" role="img" aria-labelledby="title description">
<title id="title">MCFinder</title>
<desc id="description">Original block-letter MCFinder wordmark with stone faces, dark extruded edges and a subtle moving highlight.</desc>
<defs>
  <linearGradient id="stone" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="#f8faf5"/><stop offset=".42" stop-color="#d6dbd1"/>
    <stop offset=".43" stop-color="#bbc3b5"/><stop offset="1" stop-color="#8e9c83"/>
  </linearGradient>
  <linearGradient id="shine">
    <stop offset="0" stop-color="#fff" stop-opacity="0"/>
    <stop offset=".5" stop-color="#fff" stop-opacity=".34"/>
    <stop offset="1" stop-color="#fff" stop-opacity="0"/>
  </linearGradient>
  <g id="letters">{paths}</g>
  <clipPath id="letter-clip"><use href="#letters"/></clipPath>
</defs>
<style>
  .float {{ animation: hover 5s ease-in-out infinite; transform-origin: 500px 130px; }}
  .shine {{ animation: sweep 8s ease-in-out infinite; }}
  .spark {{ animation: twinkle 4s ease-in-out infinite; }}
  @keyframes hover {{ 0%,100% {{transform:translateY(0)}} 50% {{transform:translateY(-3px)}} }}
  @keyframes sweep {{ 0%,35% {{transform:translateX(-220px)}} 75%,100% {{transform:translateX(1120px)}} }}
  @keyframes twinkle {{ 0%,100% {{opacity:.25}} 50% {{opacity:.85}} }}
  @media (prefers-reduced-motion: reduce) {{ .float,.shine,.spark {{animation:none}} .shine {{display:none}} }}
</style>
<g class="float">
  <g transform="translate({start+2} 73)">
    <use href="#letters" transform="translate(10 14)" fill="#18241c" stroke="#111b17" stroke-width="10" stroke-linejoin="miter"/>
    <use href="#letters" transform="translate(7 10)" fill="#354a32" stroke="#1b2b20" stroke-width="6"/>
    <use href="#letters" transform="translate(3 5)" fill="#53674a" stroke="#243629" stroke-width="5"/>
    <use href="#letters" fill="url(#stone)" stroke="#26382b" stroke-width="4" stroke-linejoin="miter" paint-order="stroke fill"/>
    <g clip-path="url(#letter-clip)" fill="none" stroke="#fff" stroke-opacity=".30" stroke-width="2">
      <use href="#letters" transform="translate(0 2)"/>
    </g>
    <g clip-path="url(#letter-clip)"><rect class="shine" x="-160" y="0" width="135" height="130" fill="url(#shine)"/></g>
  </g>
  <g fill="#78b84e" class="spark">
    <path d="M51 136h7v7h-7z M61 121h4v4h-4z M940 119h6v6h-6z M929 137h4v4h-4z"/>
  </g>
</g>
</svg>
'''
root=Path(__file__).resolve().parents[1]
(root/'docs/assets/mcfinder-wordmark.svg').write_text(svg)
print('docs/assets/mcfinder-wordmark.svg')
