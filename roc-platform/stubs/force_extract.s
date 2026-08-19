# Always-linked object that turns weak references into strong ones.
#
# Every host reference to __cxa_pure_virtual is weak, and a weak undefined
# reference does not extract an archive member, so libstdc++.a never provides
# it and the linker resolves it to address 0. A pure-virtual dispatch then
# jumps to null. The strong reference here extracts the definition.

.section .rodata.force_extract
.balign 8
.quad __cxa_pure_virtual
