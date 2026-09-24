;redcode-94
;name Seed Noise 2
;author board-corewar seeds
;strategy Eight random instructions (Python random, seed 2026), kept as generated.
;assert CORESIZE == 8000
        ADD.AB  *4, $-5
        SNE.I   {6, }12
        SNE.I   }-5, #7
        ADD.AB  $-3, $2
        MOV.I   }9, *-6
        JMZ.B   >-1, *0
        SEQ.I   $11, *-10
        SEQ.I   >-3, }-8
