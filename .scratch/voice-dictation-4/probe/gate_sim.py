import struct, math, glob, os, sys
def read_wav(f):
    b=open(f,"rb").read(); i=12; sr=16000; fmt=1; bits=16; ch=1
    while i<len(b):
        cid=b[i:i+4]; sz=struct.unpack("<I",b[i+4:i+8])[0]
        if cid==b"fmt ":
            fmt,ch,sr=struct.unpack("<HHI",b[i+8:i+16]); bits=struct.unpack("<H",b[i+22:i+24])[0]
        if cid==b"data": data=b[i+8:i+8+sz]; break
        i+=8+sz+(sz&1)
    if bits==32: s=struct.unpack("<%df"%(len(data)//4),data)
    else: s=[x/32768 for x in struct.unpack("<%dh"%(len(data)//2),data)]
    if ch>1: s=s[::ch]
    return sr,list(s)
def db(x): return 20*math.log10(max(x,1e-7))
FRAME=0.02; OPEN_FRAMES=int(sys.argv[1]) if len(sys.argv)>1 else 5; CLOSE_S=float(sys.argv[2]) if len(sys.argv)>2 else 1.5
ABOVE=float(sys.argv[3]) if len(sys.argv)>3 else 10.0; OPEN_MIN=float(sys.argv[4]) if len(sys.argv)>4 else -50.0
FLOOR_MIN=-70.0; RISE_PER_S=3.0; MARGIN=0.3
def simulate(levels):
    floor=FLOOR_MIN; open_=True; run=0; silent=0; events=[("open",0.0)]
    for i,lv in enumerate(levels):
        floor=max(FLOOR_MIN, min(lv, floor+RISE_PER_S*FRAME))
        thr=max(floor+ABOVE, OPEN_MIN)
        if lv>=thr:
            run+=1; silent=0
            if not open_ and run>=OPEN_FRAMES:
                open_=True; events.append(("open", max(0,(i-OPEN_FRAMES+1)*FRAME-MARGIN)))
        else:
            run=0; silent+=1
            if open_ and silent*FRAME>=CLOSE_S:
                open_=False; events.append(("close", (i-silent+1)*FRAME+MARGIN))
    return events
sess=os.path.join(os.environ["LOCALAPPDATA"],"Zed","dictation","audio")
handy=os.path.join(os.environ["APPDATA"],"com.pais.handy","recordings")
files=sorted(glob.glob(os.path.join(sess,"*.wav")),key=os.path.getmtime)+sorted(glob.glob(os.path.join(handy,"*.wav")))
for f in files:
    sr,s=read_wav(f); frame=int(sr*FRAME)
    levels=[db(math.sqrt(sum(x*x for x in s[k:k+frame])/frame)) for k in range(0,len(s)-frame+1,frame)]
    ev=simulate(levels)
    print("%-45s dur %5.1f  %s" % (os.path.basename(f)[:44], len(s)/sr, "  ".join("%s@%.2f"%e for e in ev)))
