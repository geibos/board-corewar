;redcode-94
;name Seed Noise 1
;author board-corewar seeds
;strategy Eight random instructions (Python random, seed 1994), kept as generated.
;assert CORESIZE == 8000
        JMN.B   }8, >10
        JMZ.B   *7, @-12
        DJN.B   >-4, *-3
        MOV.I   *6, #-12
        ADD.AB  {4, @5
        DAT.F   }3, @-7
        JMN.B   >0, {4
        DAT.F   @-4, #-7
