import math, json, sys
def hex2rgb(h): h=h.lstrip('#'); return tuple(int(h[i:i+2],16)/255 for i in (0,2,4))
def rgb2hex(r,g,b): return '#%02x%02x%02x' % tuple(max(0,min(255,round(c*255))) for c in (r,g,b))
def lin(c): return c/12.92 if c<=0.04045 else ((c+0.055)/1.055)**2.4
def gam(c): return 12.92*c if c<=0.0031308 else 1.055*c**(1/2.4)-0.055
def rgb2oklab(r,g,b):
    r,g,b=lin(r),lin(g),lin(b)
    l=0.4122214708*r+0.5363325363*g+0.0514459929*b
    m=0.2119034982*r+0.6806995451*g+0.1073969566*b
    s=0.0883024619*r+0.2817188376*g+0.6299787005*b
    l,m,s=l**(1/3),m**(1/3),s**(1/3)
    return (0.2104542553*l+0.7936177850*m-0.0040720468*s, 1.9779984951*l-2.4285922050*m+0.4505937099*s, 0.0259040371*l+0.7827717662*m-0.8086757660*s)
def oklab2rgb(L,a,b):
    l=L+0.3963377774*a+0.2158037573*b; m=L-0.1055613458*a-0.0638541728*b; s=L-0.0894841775*a-1.2914855480*b
    l,m,s=l**3,m**3,s**3
    r=4.0767416621*l-3.3077115913*m+0.2309699292*s; g=-1.2684380046*l+2.6097574011*m-0.3413193965*s; bb=-0.0041960863*l-0.7034186147*m+1.7076147010*s
    return tuple(gam(max(0,min(1,c))) for c in (r,g,bb))
def hex2oklch(h):
    L,a,b=rgb2oklab(*hex2rgb(h)); return L, math.hypot(a,b), (math.degrees(math.atan2(b,a))%360)
def oklch2hex(L,C,H):
    a=C*math.cos(math.radians(H)); b=C*math.sin(math.radians(H)); return rgb2hex(*oklab2rgb(L,a,b))
def lum(h):
    r,g,b=(lin(c) for c in hex2rgb(h)); return 0.2126*r+0.7152*g+0.0722*b
def contrast(a,b):
    la,lb=lum(a),lum(b); hi,lo=max(la,lb),min(la,lb); return (hi+0.05)/(lo+0.05)
if __name__=='__main__':
    tokens=json.load(open(sys.argv[1]))
    for pair,t in tokens.items():
        for role in ('accent','accent-soft','warning','danger','fig-line','text','canvas','surface'):
            L,C,H=hex2oklch(t[role]); print(f"{pair:15} {role:12} {t[role]} L={L:.3f} C={C:.3f} H={H:5.1f}")
        print()
