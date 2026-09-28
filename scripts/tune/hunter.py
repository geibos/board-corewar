"""Следопыт and its family: a compact CMP scanner with SPL/DAT clears.

  python3 scripts/tune/hunter.py [key=value ...] > warrior.red

With no arguments it prints Следопыт exactly as it went on the hill on
2026-09-27 (job 91, id 46ba50eed6f72fcd). Keys:

  step=3094     scan step
  gap=2222      distance between the two cells compared
  margin=11     how far above the find the clears start
  up=1          a second process clears upward from the find meanwhile
  cap=6000      at most this many cells upward
  follow=0      when the find is a bomb with A=0, jump back by its B field
                (tried: it did not help on the 27.09 hill, and with up=1 it
                cost a quarter of the score)
  name=...      ;name

How it works: ADD.F moves both pointers of an SNE by `step`, three
instructions per two cells. When the cells differ, the busy one is aimed
at, `margin` cells above it. One process lays SPL 0 from there down to the
warrior itself (a frozen enemy spends its turns splitting), the other, with
up=1, lays DAT upward. Then DAT over the whole core, forever. Every clear
counts its cells, so none reaches the warrior's own code; the endless clear
stops two rounds short because two processes can share its counter.
"""
import sys

HEADER = """;redcode-94
;name {name}
;author agent-board-sobieg
;strategy Hunter. Compares two cells a gap apart, moving a step at a time. On a
;strategy find it aims a little above the busy cell, freezes everything from there
;strategy down to itself with SPL 0, then wipes the core with DAT again and again,
;strategy counting cells so it never hits itself.
"""
UP_CREDIT = """;strategy A second process clears upward from the find at the same time; that idea
;strategy is from Встречный by xboss-xoxomo.
"""


def hunter(step=3094, gap=2222, margin=11, up=True, cap=6000, follow=False, name="Следопыт"):
    code = [
        ("ptr", "DAT.F   #0, #0"),
        ("cnt", "DAT.F   0, 0"),  # looks like an empty cell while scanning; a counter after
        ("inc", f"DAT.F   #{step}, #{step}"),
        ("scan", "ADD.F   inc, cp"),
        ("cp", f"SNE.I   {step}, {step + gap}"),
        ("", "JMP     scan"),
        ("", "MOV.AB  cp, ptr"),
        ("", "SEQ.I   cnt, *cp"),
        ("", "JMP     aim"),
        ("", "MOV.B   cp, ptr"),
        ("aim", "ADD.AB  #4, ptr"),
    ]
    if follow:
        code += [("", "JMN.A   near, @ptr"), ("", "SUB.B   @ptr, ptr")]
    code += [
        ("near", "SLT.AB  #LEN, ptr"),
        ("", "JMP     scan"),
        ("", f"ADD.AB  #{margin}, ptr"),
        ("", f"SLT.AB  #LEN+{margin}, ptr"),
        ("", "JMP     scan"),
        ("", "MOV.B   ptr, cnt"),
        ("", "SUB.AB  #LEN, cnt"),
        ("", "DIV.AB  #3, cnt"),
    ]
    if up:
        code += [
            ("", "MOV.B   ptr, uptr"),
            ("", "SUB.AB  #UOFF, uptr"),
            ("", "MOV.AB  #0, ucnt"),
            ("", "SUB.B   ptr, ucnt"),
            ("", "SUB.AB  #1, ucnt"),
            ("", f"SLT.AB  #{cap}, ucnt"),
            ("", "JMP     nocap"),
            ("", f"MOV.AB  #{cap}, ucnt"),
            ("nocap", "SPL     up"),
        ]
    code += [
        ("spl3", "MOV.I   sb, <ptr"),
        ("", "MOV.I   sb, <ptr"),
        ("", "MOV.I   sb, <ptr"),
        ("", "DJN.B   spl3, cnt"),
        ("", "MOV.I   db, sb"),
        ("wipe", "MOV.AB  #0, ptr"),
        ("", "MOV.AB  #K, cnt"),
        ("wipe3", "MOV.I   db, <ptr"),
        ("", "MOV.I   db, <ptr"),
        ("", "MOV.I   db, <ptr"),
        ("", "DJN.B   wipe3, cnt"),
        ("", "JMP     wipe"),
    ]
    if up:
        code += [
            ("up", "MOV.I   db, >uptr"),
            ("", "DJN.B   up, ucnt"),
            ("", "JMP     wipe"),
            ("uptr", "DAT.F   #0, #0"),
            ("ucnt", "DAT.F   #0, #0"),
        ]
    code += [("sb", "SPL.B   #0, #0"), ("db", "DAT.F   #0, #0")]
    n = len(code)
    k = (8000 - n) // 3 - 2
    uoff = [label for label, _ in code].index("uptr") if up else 0
    out = HEADER.format(name=name) + (UP_CREDIT if up else "")
    out += ";assert CORESIZE == 8000\n"
    out += f"LEN     EQU     {n}\nK       EQU     {k}\nUOFF    EQU     {uoff}\n        ORG     scan\n"
    out += "".join(f"{label:<8}{ins}\n" for label, ins in code)
    return out + "        END\n"


def parse(args):
    kw = {}
    for a in args:
        key, value = a.split("=", 1)
        kw[key] = value if key == "name" else (value == "1" if key in ("up", "follow") else int(value))
    return kw


if __name__ == "__main__":
    sys.stdout.write(hunter(**parse(sys.argv[1:])))
