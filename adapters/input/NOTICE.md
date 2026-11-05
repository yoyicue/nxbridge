# Input adapter provenance

NXBridge input bridge and patch application scripts use the root MIT license,
except for upstream-derived material listed below. Patch context retains its
upstream copyright and license; the root MIT license does not replace it.

- Moonlight Android: [revision b48494c](https://github.com/moonlight-stream/moonlight-android/tree/b48494cb96bff23d8886c4775cc4f39a1075495d), GPL version 3. `moonlight-pad-layout.patch` modifies its virtual controller. Changes to GPL-covered client code are distributed under GPL-3.0-only. Scripts check revisions and require earlier experiment state; this is not a complete reproducible APK build.
- Sunshine: [revision 63d35f7](https://github.com/LizardByte/Sunshine/tree/63d35f702ee9e362e43263742981836ec0710384), GPL version 3. `apply_sunshine.py` and `sunshine_bridge.hpp` provide the experimental integration; additions to the combined Sunshine program are distributed under GPL-3.0-only.
- nxnative-side C input code is NXBridge's session-scoped adapter. The separate nxnative patches retain that project's MIT notice in `../nxnative/LICENSE.upstream`.

The GPL version 3 text is included in [LICENSES/GPL-3.0-only.txt](../../LICENSES/GPL-3.0-only.txt).
No APK, Sunshine binary, game, key, or player save is included in this source preview.
