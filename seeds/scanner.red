;redcode-94
;name Seed Scanner
;author board-corewar seeds
;strategy Looks at every seventh cell; bombs the first one that is not empty.
;assert CORESIZE == 8000
top     ADD.AB  #7, ptr
ptr     JMZ.F   top, 7
        MOV.I   bomb, @ptr
        JMP     top
bomb    DAT.F   #0, #0
