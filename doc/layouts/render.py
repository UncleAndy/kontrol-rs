from PIL import Image, ImageDraw, ImageFont
from pathlib import Path
import zipfile

ROOT=Path(__file__).parent
OUT=ROOT/'screens'
OUT.mkdir(exist_ok=True)
# Native 5x7 bitmap glyphs. No image resampling or antialiasing.
raw={
'A':'01110 10001 10001 11111 10001 10001 10001','B':'11110 10001 10001 11110 10001 10001 11110',
'C':'01111 10000 10000 10000 10000 10000 01111','D':'11110 10001 10001 10001 10001 10001 11110',
'E':'11111 10000 10000 11110 10000 10000 11111','F':'11111 10000 10000 11110 10000 10000 10000',
'G':'01111 10000 10000 10111 10001 10001 01111','H':'10001 10001 10001 11111 10001 10001 10001',
'I':'111 010 010 010 010 010 111','J':'00111 00010 00010 00010 10010 10010 01100',
'K':'10001 10010 10100 11000 10100 10010 10001','L':'10000 10000 10000 10000 10000 10000 11111',
'M':'10001 11011 10101 10101 10001 10001 10001','N':'10001 11001 10101 10011 10001 10001 10001',
'O':'01110 10001 10001 10001 10001 10001 01110','P':'11110 10001 10001 11110 10000 10000 10000',
'Q':'01110 10001 10001 10001 10101 10010 01101','R':'11110 10001 10001 11110 10100 10010 10001',
'S':'01111 10000 10000 01110 00001 00001 11110','T':'11111 00100 00100 00100 00100 00100 00100',
'U':'10001 10001 10001 10001 10001 10001 01110','V':'10001 10001 10001 10001 10001 01010 00100',
'W':'10001 10001 10001 10101 10101 10101 01010','X':'10001 10001 01010 00100 01010 10001 10001',
'Y':'10001 10001 01010 00100 00100 00100 00100','Z':'11111 00001 00010 00100 01000 10000 11111',
'0':'01110 10001 10001 10001 10001 10001 01110','1':'00100 01100 00100 00100 00100 00100 01110',
'2':'01110 10001 00001 00010 00100 01000 11111','3':'11110 00001 00001 01110 00001 00001 11110',
'4':'00010 00110 01010 10010 11111 00010 00010','5':'11111 10000 10000 11110 00001 00001 11110',
'6':'01110 10000 10000 11110 10001 10001 01110','7':'11111 00001 00010 00100 01000 01000 01000',
'8':'01110 10001 10001 01110 10001 10001 01110','9':'01110 10001 10001 01111 00001 00001 01110',
'-':'000 000 000 111 000 000 000','+':'00000 00100 00100 11111 00100 00100 00000',
'.':'0 0 0 0 0 1 1',':':'0 1 1 0 1 1 0','/':'00001 00001 00010 00100 01000 10000 10000',
'%':'11001 11010 00100 00100 01000 10110 00110','>':'000 100 010 001 010 100 000',
' ':'000 000 000 000 000 000 000','k':'00000 10000 10010 10100 11000 10100 10010',
'd':'00001 00001 01101 10011 10001 10011 01101','B':'11110 10001 10001 11110 10001 10001 11110',
'm':'00000 00000 11010 10101 10101 10101 10101','s':'00000 00000 01111 10000 01110 00001 11110'}
glyph={k:v.split() for k,v in raw.items()}
def width(s,scale=1): return sum((len(glyph[c][0])+1)*scale for c in s)-scale
def text(im,x,y,s,scale=1,color=1):
    assert x>=0 and x+width(s,scale)<=128,(s,x,width(s,scale))
    assert y>=0 and y+7*scale<=32,(s,y,scale)
    d=ImageDraw.Draw(im)
    for c in s:
        rows=glyph[c]
        for yy,row in enumerate(rows):
            for xx,p in enumerate(row):
                if p=='1': d.rectangle((x+xx*scale,y+yy*scale,x+(xx+1)*scale-1,y+(yy+1)*scale-1),fill=color)
        x+=(len(rows[0])+1)*scale
def right(im,s,y=1,scale=1): text(im,126-width(s,scale),y,s,scale)
def base(tag='',track='03 BASS'):
    im=Image.new('1',(128,32));text(im,2,1,track)
    if tag:right(im,tag)
    ImageDraw.Draw(im).line((2,10,125,10),fill=1)
    return im
items=[]
def save(im,name,title):
    im.save(OUT/(name+'.png'));items.append((name,title,im))
def bar(im,v,bipolar=False):
    d=ImageDraw.Draw(im);d.line((2,29,125,29),fill=1)
    d.line((2,27,2,31),fill=1);d.line((125,27,125,31),fill=1)
    if bipolar:
        c=64;d.line((c,26,c,31),fill=1);end=round(c+v*60)
        if end!=c:d.rectangle((min(c,end),28,max(c,end),30),fill=1)
    else:
        end=2+round(v*123)
        d.rectangle((2,28,end,30),fill=1)
def badge(im,s,x,y,active):
    d=ImageDraw.Draw(im);w=width(s)+4
    if active:d.rectangle((x,y,x+w-1,y+8),fill=1)
    text(im,x+2,y+1,s,color=0 if active else 1)
for name,label in [('play','PLAY'),('pause','PAUSE'),('armed','ARM'),('record','REC')]:
    im=base(label);d=ImageDraw.Draw(im)
    x=126-width(label)-9
    if name=='play':d.polygon([(x,1),(x,7),(x+5,4)],fill=1)
    elif name=='pause':d.rectangle((x,1,x+1,7),fill=1);d.rectangle((x+4,1,x+5,7),fill=1)
    else:d.ellipse((x,1,x+6,7),outline=1,fill=1 if name=='record' else 0)
    text(im,2,15,'01:23.4',2);badge(im,'SOLO',98,12,True);badge(im,'MUTE',98,23,False)
    save(im,'01_main_'+name,'MAIN / '+label)
for name,track,gain,v in [('02_volume','03 BASS','-6.0',.65),('03_master','MASTER','-3.0',.8)]:
    im=base('VOL',track);text(im,2,13,gain,2);text(im,54,19,'dB');bar(im,v);save(im,name,'TRACK VOLUME' if name.startswith('02') else 'MASTER VOLUME')
im=base('PAN');text(im,2,13,'L 25',2);bar(im,-.25,True);save(im,'04_pan','TRACK PAN')
im=base('EQ 1-8');d=ImageDraw.Draw(im);zero=21
d.line((2,zero,125,zero),fill=1)
for i,g in enumerate([-3,2,5,0,-4,3,6,-2]):
    x=5+i*15;y=zero-round(g*1.5)
    if g:d.rectangle((x,min(y,zero),x+8,max(y,zero)),fill=1)
    else:d.rectangle((x,zero-1,x+8,zero+1),fill=1)
    d.point((x+4,31),fill=1)
save(im,'05_eq8','EIGHT-BAND EQ')
for name,band,freq,gain,v in [('06_eq_boost','EQ 3','1.2','+3.0',.25),('07_eq_cut','EQ 6','6.4','-4.5',-.375)]:
    im=base(band);text(im,2,13,freq,2);text(im,34,19,'k');text(im,52,13,gain,2);text(im,108,19,'dB');bar(im,v,True);save(im,name,'EQ BAND / '+gain+' dB')
im=base('FX 2/4');text(im,2,13,'REACOMP',2);save(im,'08_fx','FX SELECT')
# Plugin status is in the header of a separate bypass view.
im=base('FX ON');text(im,2,13,'REACOMP',2);save(im,'09_fx_enabled','FX ENABLED')
im=base('SEND');text(im,2,13,'-12',2);text(im,40,19,'dB');right(im,'REVERB',16);bar(im,.35);save(im,'10_send','SEND / REVERB')
im=base('AUTO');text(im,2,13,'TOUCH',2);save(im,'11_automation','AUTOMATION')
im=base('COMP');text(im,2,13,'4:1',2);text(im,56,14,'THR -18');text(im,56,23,'GR');d=ImageDraw.Draw(im)
for i in range(10):d.rectangle((73+i*5,24,75+i*5,29),fill=1 if i<4 else 0,outline=1)
save(im,'12_compressor','COMPRESSOR')
im=base('DELAY');text(im,2,13,'1/8',2);text(im,56,13,'FB 35%');text(im,56,23,'MIX 20%');save(im,'13_delay','DELAY')
im=base('CUTOFF');text(im,2,13,'2.4',2);text(im,40,19,'k');text(im,50,19,'HZ');bar(im,.6);save(im,'14_cutoff','FILTER CUTOFF')
im=base('MIDI');text(im,2,13,'Am7',2);text(im,52,13,'VEL 96');text(im,52,23,'OCT +1');save(im,'15_chord','MIDI / CHORD')
im=base('METERS');text(im,2,15,'L');text(im,2,25,'R');d=ImageDraw.Draw(im)
for row,n in [(15,16),(25,13)]:
    for i in range(20):
        x=14+i*5
        if i<n:d.rectangle((x,row,x+2,row+5),fill=1)
        else:d.point((x+1,row+5),fill=1)
save(im,'16_meters','TRACK LEVEL METERS')
# Comparison sheet: exactly 6x nearest-neighbor previews and captions outside UI.
scale=6;cw=824;ch=254;rows=(len(items)+1)//2
sheet=Image.new('RGB',(2*cw+64,rows*ch+96),'#101419');sd=ImageDraw.Draw(sheet)
font=ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf',16)
titlefont=ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf',26)
sd.text((32,20),'REAPER  /  128 x 32  /  MONOCHROME INTERFACE',font=titlefont,fill='#e9eef4')
for i,(name,title,im) in enumerate(items):
    x=32+(i%2)*cw;y=82+(i//2)*ch
    sd.text((x,y),title,font=font,fill='#9eacbb')
    sheet.paste(im.resize((768,192),Image.Resampling.NEAREST).convert('RGB'),(x,y+30))
sheet.save(ROOT/'reaper_pro_overview.png')
for p in OUT.glob('*.png'):
    im=Image.open(p);assert im.size==(128,32) and im.mode=='1'
with zipfile.ZipFile(ROOT/'reaper_pro_128x32.zip','w',zipfile.ZIP_DEFLATED) as z:
    for p in sorted(OUT.glob('*.png')):z.write(p,'screens/'+p.name)
    z.write(ROOT/'reaper_pro_overview.png','reaper_pro_overview.png')
    z.write(Path(__file__),'render.py')
print(f'Validated {len(items)} native 128x32 one-bit screens')
