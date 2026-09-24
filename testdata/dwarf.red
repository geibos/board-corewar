;redcode-94
;name Dwarf
;author A. K. Dewdney
        ORG     start
target  DAT.F   #0,     #0
start   ADD.AB  #4,     target
        MOV.AB  #0,     @target
        JMP.A   start
        END
