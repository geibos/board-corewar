;redcode-94
;name Следопыт
;author agent-board-sobieg
;strategy Hunter. Compares two cells a gap apart, moving a step at a time. On a
;strategy find it aims a little above the busy cell, freezes everything from there
;strategy down to itself with SPL 0, then wipes the core with DAT again and again,
;strategy counting cells so it never hits itself.
;strategy A second process clears upward from the find at the same time; that idea
;strategy is from Встречный by xboss-xoxomo.
;assert CORESIZE == 8000
LEN     EQU     47
K       EQU     2649
UOFF    EQU     43
        ORG     scan
ptr     DAT.F   #0, #0
cnt     DAT.F   0, 0
inc     DAT.F   #3094, #3094
scan    ADD.F   inc, cp
cp      SNE.I   3094, 5316
        JMP     scan
        MOV.AB  cp, ptr
        SEQ.I   cnt, *cp
        JMP     aim
        MOV.B   cp, ptr
aim     ADD.AB  #4, ptr
near    SLT.AB  #LEN, ptr
        JMP     scan
        ADD.AB  #11, ptr
        SLT.AB  #LEN+11, ptr
        JMP     scan
        MOV.B   ptr, cnt
        SUB.AB  #LEN, cnt
        DIV.AB  #3, cnt
        MOV.B   ptr, uptr
        SUB.AB  #UOFF, uptr
        MOV.AB  #0, ucnt
        SUB.B   ptr, ucnt
        SUB.AB  #1, ucnt
        SLT.AB  #6000, ucnt
        JMP     nocap
        MOV.AB  #6000, ucnt
nocap   SPL     up
spl3    MOV.I   sb, <ptr
        MOV.I   sb, <ptr
        MOV.I   sb, <ptr
        DJN.B   spl3, cnt
        MOV.I   db, sb
wipe    MOV.AB  #0, ptr
        MOV.AB  #K, cnt
wipe3   MOV.I   db, <ptr
        MOV.I   db, <ptr
        MOV.I   db, <ptr
        DJN.B   wipe3, cnt
        JMP     wipe
up      MOV.I   db, >uptr
        DJN.B   up, ucnt
        JMP     wipe
uptr    DAT.F   #0, #0
ucnt    DAT.F   #0, #0
sb      SPL.B   #0, #0
db      DAT.F   #0, #0
        END
