import json, sys
sys.path.insert(0,'.')
from color import *
tokens=json.load(open('lab/tokens.json'))
plan={'graphite':(262,315),'slate':(215,295),'sage':(250,320)}
out={}
for pair,t in tokens.items():
    skin=pair.split('-')[0]; mode=pair.split('-')[1]
    La,Ca,Ha=hex2oklch(t['accent']); Ls,Cs,Hs=hex2oklch(t['accent-soft'])
    Cink=max(Ca,0.10);
    row={}
    for i,h in enumerate(plan[skin],1):
        ink=oklch2hex(La,Cink,h); fill=oklch2hex(Ls,max(Cs,0.025),h)
        row[f'fig-cat{i}']=ink; row[f'fig-cat{i}-soft']=fill
    out[pair]=row
    g=t['surface']; cv=t['canvas']; ink=t['fig-line']
    print(pair, 'accent', t['accent'], 'vs ground %.2f'%contrast(t['accent'],g), 'soft vs ground %.2f'%contrast(t['accent-soft'],g), 'ink on soft %.2f'%contrast(ink,t['accent-soft']))
    for k,v in row.items():
        if k.endswith('soft'):
            print('   ',k,v,'vs ground %.2f'%contrast(v,g),'ink on it %.2f'%contrast(ink,v), 'cat ink on it %.2f'%contrast(row[k[:-5]],v))
        else:
            print('   ',k,v,'vs ground %.2f'%contrast(v,g),'vs canvas %.2f'%contrast(v,cv), 'vs accent %.2f'%contrast(v,t['accent']))
json.dump(out,open('cat-tokens.json','w'),indent=1)
