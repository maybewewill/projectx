"""Debug helper: records the last executed addresses so a fault can be located. Usage: import and call arm()."""
from unicorn import UC_HOOK_CODE
import emu

LAST = []


def arm(n=40):
    def cb(uc, addr, size, user):
        LAST.append(addr)
        if len(LAST) > n:
            del LAST[0]
    emu.mu.hook_add(UC_HOOK_CODE, cb)


def dump():
    print(" ".join("%x" % a for a in LAST))
