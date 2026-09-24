;redcode-94
;name registers
;author board-corewar tests
;assert 1
;strategy pMARS sets register W (the number of warriors) only while it
;strategy assembles the first warrior: first this is JMP 2 and dies on the
;strategy DAT, second it is JMP 0 and lives.
        JMP W
        DAT 0, 0
        DAT 0, 0
