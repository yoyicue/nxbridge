# NXBridge owns input and offscreen presentation in this isolated build.
ifeq ($(S2A_MAC_FRONTEND),1)
$(error The NXBridge S2Arm adapter requires a headless host; omit S2A_MAC_FRONTEND)
endif
