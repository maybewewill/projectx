//! Presentation natives: camera, sound and music, text and text tags, quests, multiboards,
//! leaderboards, dialogs, timer dialogs, cinematic filters and scenes, fog and sky visuals, special
//! effects, lightning, images, ubersplats, weather, terrain deformation and minimap pings.
//!
//! These natives do not change the simulation. What a script can read back is kept here (camera
//! values, the texts and sizes of UI objects, the state of the handle objects), so the getters
//! answer what the script set. What a renderer or a sound system would show is written to
//! [`PresentationLog`] as [`PresentationEvent`]s. Nothing is drawn or played here, and the timed
//! parts (fades, durations, camera transitions) are not simulated: the stored values are the ones
//! the script wrote, and a host that renders them may finish the transitions.
//!
//! Handles. A creator gives its handle in one of two ways (the table at the end says which for each
//! native):
//! * **Handle table** (`Game::add_object` with flag 0 or 1). The id is the one the handle table
//!   gives (base 0x100000, LIFO reuse of freed ids). Used by camera setups, sounds, effects, quests,
//!   quest items, multiboards and their items, leaderboards, dialogs, buttons and timer dialogs. The
//!   flag is the one the original passes to its handle allocator (`FUN_6f430c80`), read from
//!   `natives.c`.
//! * **Pool of this module**. Texttags, lightnings, images, ubersplats, weather effects and terrain
//!   deformations are not in the handle table in the original. Their handle value is the pool slot
//!   plus one, so 0 is the null handle, and a freed slot is reused LIFO. The numbering of the pools
//!   is UNSURE (their allocator, `FUN_6f4e5760`, is not read).
//!
//! Getters of state that is not set by the script return the default written next to the field.
//! Defaults that depend on the renderer, the map or the game data are marked UNSURE.
//!
//! Natives are registered by [`register`]. Addresses are the 1.26a `Game.dll` natives
//! (`docs/spec/natives-1.26a.tsv`).
//!
//! Status: CONFIRMED = read in the decompile of the native (`natives.c`) or of the helper it
//! calls; LIKELY = strong inference from the code shape; UNSURE = guessed or not read. The
//! return column says what the native hands back; "stored value" means the getter reads back
//! what the script set. Handle allocation is in the handle or pool column.
//!
//! | Native | Address | Handle allocation | Status | Return |
//! |---|---|---|---|---|
//! | `AddLightning` | 0x6f3bcc20 | lightning pool slot | CONFIRMED return index+1, numbering UNSURE | handle (see alloc) |
//! | `AddLightningEx` | 0x6f3bcb30 | lightning pool slot | CONFIRMED return index+1, numbering UNSURE | handle (see alloc) |
//! | `AddSpecialEffect` | 0x6f3bca90 | flag 0 (effect helper FUN_6f3bca30) | CONFIRMED | handle (see alloc) |
//! | `AddSpecialEffectLoc` | 0x6f3cd260 | flag 0 (effect helper FUN_6f3bca30) | CONFIRMED | handle (see alloc) |
//! | `AddSpecialEffectTarget` | 0x6f3cd2a0 | flag 0 (helper FUN_6f3cd120 not read) | LIKELY | handle (see alloc) |
//! | `AddSpellEffect` | 0x6f3bcb10 | flag 0 (effect helper FUN_6f3bca30) | CONFIRMED | handle (see alloc) |
//! | `AddSpellEffectById` | 0x6f3bcae0 | flag 0 (effect helper FUN_6f3bca30) | CONFIRMED | handle (see alloc) |
//! | `AddSpellEffectByIdLoc` | 0x6f3cd2d0 | flag 0 (effect helper FUN_6f3bca30) | CONFIRMED | handle (see alloc) |
//! | `AddSpellEffectLoc` | 0x6f3cd320 | flag 0 (effect helper FUN_6f3bca30) | CONFIRMED | handle (see alloc) |
//! | `AddSpellEffectTarget` | 0x6f3cd4a0 | none | - | CONFIRMED null (helper returns 0) |
//! | `AddSpellEffectTargetById` | 0x6f3cd380 | none | - | CONFIRMED null (helper returns 0) |
//! | `AddWeatherEffect` | 0x6f3ccfe0 | weather pool slot | UNSURE (void decompile) | handle (see alloc) |
//! | `AdjustCameraField` | 0x6f3b48f0 | none | - | - |
//! | `AttachSoundToUnit` | 0x6f3cce50 | none | - | - |
//! | `CameraSetSmoothingFactor` | 0x6f3b4bb0 | none | - | - |
//! | `CameraSetSourceNoise` | 0x6f3b49e0 | none | - | - |
//! | `CameraSetSourceNoiseEx` | 0x6f3b4b20 | none | - | - |
//! | `CameraSetTargetNoise` | 0x6f3b4930 | none | - | - |
//! | `CameraSetTargetNoiseEx` | 0x6f3b4a70 | none | - | - |
//! | `CameraSetupApply` | 0x6f3cd900 | none | - | - |
//! | `CameraSetupApplyForceDuration` | 0x6f3cd960 | none | - | - |
//! | `CameraSetupApplyForceDurationWithZ` | 0x6f3cd990 | none | - | - |
//! | `CameraSetupApplyWithZ` | 0x6f3cd930 | none | - | - |
//! | `CameraSetupGetDestPositionLoc` | 0x6f3d3e40 | location, flag 1 (Location) | CONFIRMED | handle (see alloc) |
//! | `CameraSetupGetDestPositionX` | 0x6f3cd8c0 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `CameraSetupGetDestPositionY` | 0x6f3cd8e0 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `CameraSetupGetField` | 0x6f3cd830 | none | - | stored; defaults UNSURE |
//! | `CameraSetupSetDestPosition` | 0x6f3cd870 | none | - | - |
//! | `CameraSetupSetField` | 0x6f3cd800 | none | - | - |
//! | `ClearMapMusic` | 0x6f3b3d20 | none | - | - |
//! | `ClearStackedSound` | 0x6f3b3c80 | none | - | - |
//! | `ClearStackedSoundRect` | 0x6f3ccb20 | none | - | - |
//! | `ClearTextMessages` | 0x6f3b4e60 | none | - | - |
//! | `CreateCameraSetup` | 0x6f3d3d40 | flag 0 (own FUN_6f430c80 call) | CONFIRMED | handle (see alloc) |
//! | `CreateImage` | 0x6f3b4320 | image pool slot | LIKELY pool, -1 null in original, numbering UNSURE | handle (see alloc) |
//! | `CreateLeaderboard` | 0x6f3d35b0 | flag 0 (own FUN_6f430c80 call) | CONFIRMED | handle (see alloc) |
//! | `CreateMIDISound` | 0x6f3d3c30 | flag 1 (own FUN_6f430c80 call) | CONFIRMED | handle (see alloc) |
//! | `CreateMultiboard` | 0x6f3d36b0 | flag 0 (own FUN_6f430c80 call) | CONFIRMED | handle (see alloc) |
//! | `CreateQuest` | 0x6f3bc970 | flag 0 (own FUN_6f430c80 call) | CONFIRMED | handle (see alloc) |
//! | `CreateSound` | 0x6f3d38d0 | flag 1 (own FUN_6f430c80 call) | CONFIRMED | handle (see alloc) |
//! | `CreateSoundFilenameWithLabel` | 0x6f3d39f0 | flag 1 (own FUN_6f430c80 call) | CONFIRMED | handle (see alloc) |
//! | `CreateSoundFromLabel` | 0x6f3d3b10 | flag 1 (own FUN_6f430c80 call) | CONFIRMED | handle (see alloc) |
//! | `CreateTextTag` | 0x6f3bc580 | text-tag pool slot | CONFIRMED pool, numbering UNSURE | handle (see alloc) |
//! | `CreateTimerDialog` | 0x6f3d34b0 | flag 0 (own FUN_6f430c80 call) | CONFIRMED | handle (see alloc) |
//! | `CreateUbersplat` | 0x6f3b85f0 | ubersplat pool slot | LIKELY pool, numbering UNSURE | handle (see alloc) |
//! | `DestroyEffect` | 0x6f3cd240 | death call, vcall 0x9c | CONFIRMED call, LIKELY release | - |
//! | `DestroyImage` | 0x6f3b43b0 | pool slot freed (LIFO) | LIKELY | - |
//! | `DestroyLeaderboard` | 0x6f3cbfd0 | death call, vcall 0x5c | CONFIRMED call, LIKELY release | - |
//! | `DestroyLightning` | 0x6f3b4150 | pool slot freed (LIFO) | LIKELY | - |
//! | `DestroyMultiboard` | 0x6f3cc650 | death call, vcall 0x5c | CONFIRMED call, LIKELY release | - |
//! | `DestroyQuest` | 0x6f3cbb20 | release (vcall not seen; helper FUN_6f3f94a0) | LIKELY | - |
//! | `DestroyTextTag` | 0x6f3bc5a0 | pool slot freed (LIFO) | LIKELY | - |
//! | `DestroyTimerDialog` | 0x6f3cbe30 | death call, vcall 0x5c | CONFIRMED call, LIKELY release | - |
//! | `DestroyUbersplat` | 0x6f3b44c0 | pool slot freed (LIFO) | LIKELY | - |
//! | `DialogAddButton` | 0x6f3c9f90 | flag 0 (own FUN_6f430c80 call) | CONFIRMED | handle (see alloc) |
//! | `DialogAddQuitButton` | 0x6f3c9fe0 | flag 0 (own FUN_6f430c80 call) | CONFIRMED | handle (see alloc) |
//! | `DialogClear` | 0x6f3c9f40 | none | - | - |
//! | `DialogCreate` | 0x6f3d2bc0 | flag 0 (own FUN_6f430c80 call) | CONFIRMED | handle (see alloc) |
//! | `DialogDestroy` | 0x6f3c9f00 | death call, vcall 0x5c; buttons released too | CONFIRMED call, UNSURE buttons | - |
//! | `DialogDisplay` | 0x6f3ca030 | none | - | - |
//! | `DialogSetAsync` | 0x6f3c9f20 | none | - | - |
//! | `DialogSetMessage` | 0x6f3c9f60 | none | - | - |
//! | `DisplayCineFilter` | 0x6f3b5120 | none | - | - |
//! | `DisplayLoadDialog` | 0x6f3b3a10 | none | - | - |
//! | `DisplayTextToPlayer` | 0x6f3cb900 | none | - | - |
//! | `DisplayTimedTextFromPlayer` | 0x6f3cba30 | none | - | - |
//! | `DisplayTimedTextToPlayer` | 0x6f3cb9b0 | none | - | - |
//! | `DoNotSaveReplay` | 0x6f3b3550 | none | - | - |
//! | `EnableDragSelect` | 0x6f3b3b30 | none | - | - |
//! | `EnableMinimapFilterButtons` | 0x6f3b3af0 | none | - | - |
//! | `EnableOcclusion` | 0x6f3b3b90 | none | - | - |
//! | `EnablePreSelect` | 0x6f3b3b50 | none | - | - |
//! | `EnableSelect` | 0x6f3b3b70 | none | - | - |
//! | `EnableUserControl` | 0x6f3b3880 | none | - | - |
//! | `EnableUserUI` | 0x6f3b38a0 | none | - | - |
//! | `EnableWeatherEffect` | 0x6f3b3e80 | none | - | - |
//! | `EnableWorldFogBoundary` | 0x6f3b3bb0 | none | - | - |
//! | `EndCinematicScene` | 0x6f3b4e20 | none | - | - |
//! | `EndThematicMusic` | 0x6f3b3de0 | none | - | - |
//! | `FinishUbersplat` | 0x6f3b44e0 | none | - | - |
//! | `FlashQuestDialogButton` | 0x6f3bc920 | none | - | - |
//! | `FogEnable` | 0x6f3bb1e0 | none | - | - |
//! | `FogMaskEnable` | 0x6f3bb210 | none | - | - |
//! | `ForceCinematicSubtitles` | 0x6f3b4e40 | none | - | - |
//! | `ForceQuestDialogUpdate` | 0x6f3b3be0 | none | - | - |
//! | `ForceUICancel` | 0x6f3b3990 | none | - | - |
//! | `ForceUIKey` | 0x6f3b38e0 | none | - | - |
//! | `GetAllyColorFilterState` | 0x6f3b3a50 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `GetCameraBoundMaxX` | 0x6f3b4c90 | none | - | LIKELY: explicit bounds, else playable area |
//! | `GetCameraBoundMaxY` | 0x6f3b4cb0 | none | - | LIKELY: explicit bounds, else playable area |
//! | `GetCameraBoundMinX` | 0x6f3b4c50 | none | - | LIKELY: explicit bounds, else playable area |
//! | `GetCameraBoundMinY` | 0x6f3b4c70 | none | - | LIKELY: explicit bounds, else playable area |
//! | `GetCameraEyePositionLoc` | 0x6f3d3ee0 | location, flag 1 (Location) | CONFIRMED | UNSURE: eye = target here |
//! | `GetCameraEyePositionX` | 0x6f3b4d90 | none | - | UNSURE: eye = target here |
//! | `GetCameraEyePositionY` | 0x6f3b4dc0 | none | - | UNSURE: eye = target here |
//! | `GetCameraEyePositionZ` | 0x6f3b4df0 | none | - | UNSURE: eye = target here |
//! | `GetCameraField` | 0x6f3b4cd0 | none | - | stored; defaults UNSURE |
//! | `GetCameraMargin` | 0x6f3b4be0 | none | - | UNSURE: screen aspect (renderer), 0 here |
//! | `GetCameraTargetPositionLoc` | 0x6f3d3e90 | location, flag 1 (Location) | CONFIRMED | handle (see alloc) |
//! | `GetCameraTargetPositionX` | 0x6f3b4d00 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `GetCameraTargetPositionY` | 0x6f3b4d30 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `GetCameraTargetPositionZ` | 0x6f3b4d60 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `GetCreepCampFilterState` | 0x6f3b3aa0 | none | - | stored; default UNSURE |
//! | `GetCustomCampaignButtonVisible` | 0x6f3b3530 | none | - | stored; default UNSURE |
//! | `GetLightningColorA` | 0x6f3b8310 | none | - | UNSURE: byte / 255 here |
//! | `GetLightningColorB` | 0x6f3b8430 | none | - | UNSURE: byte / 255 here |
//! | `GetLightningColorG` | 0x6f3b83d0 | none | - | UNSURE: byte / 255 here |
//! | `GetLightningColorR` | 0x6f3b8370 | none | - | UNSURE: byte / 255 here |
//! | `GetSoundDuration` | 0x6f3ccf20 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `GetSoundFileDuration` | 0x6f3b3e20 | none | - | host table; 0 when unknown |
//! | `GetSoundIsLoading` | 0x6f3ccf60 | none | - | LIKELY false (no streaming here) |
//! | `GetSoundIsPlaying` | 0x6f3ccf40 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `GetTimeOfDayScale` | 0x6f3bc4b0 | none | - | UNSURE default 1.0 |
//! | `IsCineFilterDisplayed` | 0x6f3b5150 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `IsFogEnabled` | 0x6f3bb200 | none | - | stored; default UNSURE |
//! | `IsFogMaskEnabled` | 0x6f3bb230 | none | - | stored; default UNSURE |
//! | `IsLeaderboardDisplayed` | 0x6f3cc220 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `IsMultiboardDisplayed` | 0x6f3cc690 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `IsMultiboardMinimized` | 0x6f3cc6d0 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `IsQuestCompleted` | 0x6f3cbcc0 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `IsQuestDiscovered` | 0x6f3cbce0 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `IsQuestEnabled` | 0x6f3cbd20 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `IsQuestFailed` | 0x6f3cbd00 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `IsQuestItemCompleted` | 0x6f3cbd90 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `IsQuestRequired` | 0x6f3cbca0 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `IsTimerDialogDisplayed` | 0x6f3cbf70 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `KillSoundWhenDone` | 0x6f3cced0 | released now if not playing, else by finish_sound | UNSURE | - |
//! | `LeaderboardAddItem` | 0x6f3cc060 | none | - | - |
//! | `LeaderboardClear` | 0x6f3cc1b0 | none | - | - |
//! | `LeaderboardDisplay` | 0x6f3cc1d0 | none | - | - |
//! | `LeaderboardGetItemCount` | 0x6f3cc000 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `LeaderboardGetLabelText` | 0x6f3cc2f0 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `LeaderboardGetPlayerIndex` | 0x6f3cc2a0 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `LeaderboardHasPlayerItem` | 0x6f3cc250 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `LeaderboardRemoveItem` | 0x6f3cc0b0 | none | - | - |
//! | `LeaderboardRemovePlayerItem` | 0x6f3cc0e0 | none | - | - |
//! | `LeaderboardSetItemLabel` | 0x6f3cc520 | none | - | - |
//! | `LeaderboardSetItemLabelColor` | 0x6f3cc5b0 | none | - | - |
//! | `LeaderboardSetItemStyle` | 0x6f3cc560 | none | - | - |
//! | `LeaderboardSetItemValue` | 0x6f3cc4f0 | none | - | - |
//! | `LeaderboardSetItemValueColor` | 0x6f3cc600 | none | - | - |
//! | `LeaderboardSetLabel` | 0x6f3cc3d0 | none | - | - |
//! | `LeaderboardSetLabelColor` | 0x6f3cc400 | none | - | - |
//! | `LeaderboardSetSizeByItemCount` | 0x6f3cc030 | none | - | - |
//! | `LeaderboardSetStyle` | 0x6f3cc4a0 | none | - | - |
//! | `LeaderboardSetValueColor` | 0x6f3cc450 | none | - | - |
//! | `LeaderboardSortItemsByLabel` | 0x6f3cc180 | none | - | - |
//! | `LeaderboardSortItemsByPlayer` | 0x6f3cc150 | none | - | - |
//! | `LeaderboardSortItemsByValue` | 0x6f3cc120 | none | - | - |
//! | `MoveLightning` | 0x6f3bcda0 | none | - | - |
//! | `MoveLightningEx` | 0x6f3bccc0 | none | - | - |
//! | `MultiboardClear` | 0x6f3cc6f0 | none | - | - |
//! | `MultiboardDisplay` | 0x6f3cc670 | none | - | - |
//! | `MultiboardGetColumnCount` | 0x6f3cc7d0 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `MultiboardGetItem` | 0x6f3d37b0 | flag 0 (cell item, reused) | LIKELY | handle (see alloc) |
//! | `MultiboardGetRowCount` | 0x6f3cc7b0 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `MultiboardGetTitleText` | 0x6f3cc750 | none | - | stored value (LIKELY: the getter reads back the field the setter writes) |
//! | `MultiboardMinimize` | 0x6f3cc6b0 | none | - | - |
//! | `MultiboardReleaseItem` | 0x6f3cc950 | death call, vcall 0x5c | CONFIRMED call, LIKELY release | - |
//! | `MultiboardSetColumnCount` | 0x6f3cc7f0 | none | - | - |
//! | `MultiboardSetItemIcon` | 0x6f3cca20 | none | - | - |
//! | `MultiboardSetItemStyle` | 0x6f3cc970 | none | - | - |
//! | `MultiboardSetItemValue` | 0x6f3cc990 | none | - | - |
//! | `MultiboardSetItemValueColor` | 0x6f3cc9c0 | none | - | - |
//! | `MultiboardSetItemWidth` | 0x6f3cca00 | none | - | - |
//! | `MultiboardSetItemsIcon` | 0x6f3cc920 | none | - | - |
//! | `MultiboardSetItemsStyle` | 0x6f3cc830 | none | - | - |
//! | `MultiboardSetItemsValue` | 0x6f3cc870 | none | - | - |
//! | `MultiboardSetItemsValueColor` | 0x6f3cc8a0 | none | - | - |
//! | `MultiboardSetItemsWidth` | 0x6f3cc8f0 | none | - | - |
//! | `MultiboardSetRowCount` | 0x6f3cc810 | none | - | - |
//! | `MultiboardSetTitleText` | 0x6f3cc720 | none | - | - |
//! | `MultiboardSetTitleTextColor` | 0x6f3cc770 | none | - | - |
//! | `MultiboardSuppressDisplay` | 0x6f3b3c00 | none | - | - |
//! | `NewSoundEnvironment` | 0x6f3b3e60 | none | - | - |
//! | `PanCameraTo` | 0x6f3b4700 | none | - | - |
//! | `PanCameraToTimed` | 0x6f3b4740 | none | - | - |
//! | `PanCameraToTimedWithZ` | 0x6f3b47d0 | none | - | - |
//! | `PanCameraToWithZ` | 0x6f3b4790 | none | - | - |
//! | `PingMinimap` | 0x6f3b4650 | none | - | - |
//! | `PingMinimapEx` | 0x6f3b8660 | none | - | - |
//! | `PlayCinematic` | 0x6f3bc520 | none | - | - |
//! | `PlayModelCinematic` | 0x6f3bc500 | none | - | - |
//! | `PlayMusic` | 0x6f3b3d30 | none | - | - |
//! | `PlayMusicEx` | 0x6f3b3d50 | none | - | - |
//! | `PlayThematicMusic` | 0x6f3b3da0 | none | - | - |
//! | `PlayThematicMusicEx` | 0x6f3b3dc0 | none | - | - |
//! | `PlayerGetLeaderboard` | 0x6f3cc380 | existing id of the leaderboard (reuse) | CONFIRMED | handle (see alloc) |
//! | `PlayerSetLeaderboard` | 0x6f3cc340 | none | - | - |
//! | `QuestCreateItem` | 0x6f3d3380 | flag 0 (own FUN_6f430c80 call) | CONFIRMED | handle (see alloc) |
//! | `QuestItemSetCompleted` | 0x6f3cbd70 | none | - | - |
//! | `QuestItemSetDescription` | 0x6f3cbd40 | none | - | - |
//! | `QuestSetCompleted` | 0x6f3cbc20 | none | - | - |
//! | `QuestSetDescription` | 0x6f3cbba0 | none | - | - |
//! | `QuestSetDiscovered` | 0x6f3cbc60 | none | - | - |
//! | `QuestSetEnabled` | 0x6f3cbc80 | none | - | - |
//! | `QuestSetFailed` | 0x6f3cbc40 | none | - | - |
//! | `QuestSetIconPath` | 0x6f3cbbd0 | none | - | - |
//! | `QuestSetRequired` | 0x6f3cbc00 | none | - | - |
//! | `QuestSetTitle` | 0x6f3cbb70 | none | - | - |
//! | `RegisterStackedSound` | 0x6f3ccf80 | none | - | - |
//! | `RemoveWeatherEffect` | 0x6f3b3e70 | pool slot freed (LIFO) | LIKELY | - |
//! | `ResetTerrainFog` | 0x6f3b3860 | none | - | - |
//! | `ResetToGameCamera` | 0x6f3b46b0 | none | - | - |
//! | `ResetUbersplat` | 0x6f3b44d0 | none | - | - |
//! | `ResumeMusic` | 0x6f3b3d90 | none | - | - |
//! | `SetAllyColorFilterState` | 0x6f3b3a70 | none | - | - |
//! | `SetAltMinimapIcon` | 0x6f3bc540 | none | - | - |
//! | `SetCameraBounds` | 0x6f3b86f0 | none | - | - |
//! | `SetCameraField` | 0x6f3b48b0 | none | - | - |
//! | `SetCameraOrientController` | 0x6f3cd7b0 | none | - | - |
//! | `SetCameraPosition` | 0x6f3b45d0 | none | - | - |
//! | `SetCameraQuickPosition` | 0x6f3b4610 | none | - | - |
//! | `SetCameraRotateMode` | 0x6f3b4850 | none | - | - |
//! | `SetCameraTargetController` | 0x6f3cd760 | none | - | - |
//! | `SetCineFilterBlendMode` | 0x6f3b4eb0 | none | - | - |
//! | `SetCineFilterDuration` | 0x6f3b5100 | none | - | - |
//! | `SetCineFilterEndColor` | 0x6f3cecd0 | none | - | - |
//! | `SetCineFilterEndUV` | 0x6f3b5020 | none | - | - |
//! | `SetCineFilterStartColor` | 0x6f3cebe0 | none | - | - |
//! | `SetCineFilterStartUV` | 0x6f3b4f40 | none | - | - |
//! | `SetCineFilterTexMapFlags` | 0x6f3b4ef0 | none | - | - |
//! | `SetCineFilterTexture` | 0x6f3b4e80 | none | - | - |
//! | `SetCinematicCamera` | 0x6f3b4820 | none | - | - |
//! | `SetCinematicScene` | 0x6f3bcf80 | none | - | - |
//! | `SetCreepCampFilterState` | 0x6f3b3ac0 | none | - | - |
//! | `SetCustomCampaignButtonVisible` | 0x6f3b3510 | none | - | - |
//! | `SetDayNightModels` | 0x6f3b3790 | none | - | - |
//! | `SetImageAboveWater` | 0x6f3b4480 | none | - | - |
//! | `SetImageColor` | 0x6f3b85b0 | none | - | - |
//! | `SetImageConstantHeight` | 0x6f3b43e0 | none | - | - |
//! | `SetImagePosition` | 0x6f3b4400 | none | - | - |
//! | `SetImageRender` | 0x6f3b4440 | none | - | - |
//! | `SetImageRenderAlways` | 0x6f3b4460 | none | - | - |
//! | `SetImageType` | 0x6f3b44a0 | none | - | - |
//! | `SetIntroShotModel` | 0x6f3bc8a0 | none | - | - |
//! | `SetIntroShotText` | 0x6f3bc880 | none | - | - |
//! | `SetLightningColor` | 0x6f3b8490 | none | - | - |
//! | `SetMapMusic` | 0x6f3b3ce0 | none | - | - |
//! | `SetMusicPlayPosition` | 0x6f3b3e00 | none | - | - |
//! | `SetMusicVolume` | 0x6f3b3df0 | none | - | - |
//! | `SetReservedLocalHeroButtons` | 0x6f3b3a30 | none | - | - |
//! | `SetSkyModel` | 0x6f3b37d0 | none | - | - |
//! | `SetSoundChannel` | 0x6f3ccc20 | none | - | - |
//! | `SetSoundConeAngles` | 0x6f3cccf0 | none | - | - |
//! | `SetSoundConeOrientation` | 0x6f3ccd30 | none | - | - |
//! | `SetSoundDistanceCutoff` | 0x6f3cccd0 | none | - | - |
//! | `SetSoundDistances` | 0x6f3ccca0 | none | - | - |
//! | `SetSoundDuration` | 0x6f3ccef0 | none | - | - |
//! | `SetSoundParamsFromLabel` | 0x6f3ccbf0 | none | - | - |
//! | `SetSoundPitch` | 0x6f3ccc60 | none | - | - |
//! | `SetSoundPlayPosition` | 0x6f3ccc80 | none | - | - |
//! | `SetSoundPosition` | 0x6f3ccd90 | none | - | - |
//! | `SetSoundVelocity` | 0x6f3ccdf0 | none | - | - |
//! | `SetSoundVolume` | 0x6f3ccc40 | none | - | - |
//! | `SetStackedSound` | 0x6f3b3c20 | none | - | - |
//! | `SetStackedSoundRect` | 0x6f3cca50 | none | - | - |
//! | `SetTerrainFog` | 0x6f3b3840 | none | - | - |
//! | `SetTerrainFogEx` | 0x6f3b81b0 | none | - | - |
//! | `SetTextTagAge` | 0x6f3bc7f0 | none | - | - |
//! | `SetTextTagColor` | 0x6f3bc6a0 | none | - | - |
//! | `SetTextTagFadepoint` | 0x6f3bc850 | none | - | - |
//! | `SetTextTagLifespan` | 0x6f3bc820 | none | - | - |
//! | `SetTextTagPermanent` | 0x6f3bc7c0 | none | - | - |
//! | `SetTextTagPos` | 0x6f3bc610 | none | - | - |
//! | `SetTextTagPosUnit` | 0x6f3cb890 | none | - | - |
//! | `SetTextTagSuspended` | 0x6f3bc790 | none | - | - |
//! | `SetTextTagText` | 0x6f3bc5d0 | none | - | - |
//! | `SetTextTagVelocity` | 0x6f3bc700 | none | - | - |
//! | `SetTextTagVisibility` | 0x6f3bc760 | none | - | - |
//! | `SetThematicMusicPlayPosition` | 0x6f3b3e10 | none | - | - |
//! | `SetTimeOfDayScale` | 0x6f3bc490 | none | - | - |
//! | `SetUbersplatRender` | 0x6f3b4510 | none | - | - |
//! | `SetUbersplatRenderAlways` | 0x6f3b4530 | none | - | - |
//! | `SetWaterBaseColor` | 0x6f3b8580 | none | - | - |
//! | `SetWaterDeforms` | 0x6f3b4240 | none | - | - |
//! | `ShowImage` | 0x6f3b43c0 | none | - | - |
//! | `ShowInterface` | 0x6f3b3810 | none | - | - |
//! | `ShowUbersplat` | 0x6f3b44f0 | none | - | - |
//! | `StartSound` | 0x6f3cce80 | none | - | - |
//! | `StopCamera` | 0x6f3b46e0 | none | - | - |
//! | `StopMusic` | 0x6f3b3d80 | none | - | - |
//! | `StopSound` | 0x6f3cceb0 | released at once when killWhenDone is set | UNSURE | - |
//! | `TerrainDeformCrater` | 0x6f3b3e90 | deformation pool slot | UNSURE (void decompile) | handle (see alloc) |
//! | `TerrainDeformRandom` | 0x6f3b4040 | deformation pool slot | UNSURE (void decompile) | handle (see alloc) |
//! | `TerrainDeformRipple` | 0x6f3b3f00 | deformation pool slot | UNSURE (void decompile) | handle (see alloc) |
//! | `TerrainDeformStop` | 0x6f3b40b0 | none | - | - |
//! | `TerrainDeformStopAll` | 0x6f3b40c0 | none | - | - |
//! | `TerrainDeformWave` | 0x6f3b3fa0 | deformation pool slot | UNSURE (void decompile) | handle (see alloc) |
//! | `TimerDialogDisplay` | 0x6f3cbf50 | none | - | - |
//! | `TimerDialogSetRealTimeRemaining` | 0x6f3cbfa0 | none | - | - |
//! | `TimerDialogSetSpeed` | 0x6f3cbf20 | none | - | - |
//! | `TimerDialogSetTimeColor` | 0x6f3cbed0 | none | - | - |
//! | `TimerDialogSetTitle` | 0x6f3cbe50 | none | - | - |
//! | `TimerDialogSetTitleColor` | 0x6f3cbe80 | none | - | - |
//! | `UnregisterStackedSound` | 0x6f3ccfb0 | none | - | - |
//! | `VolumeGroupReset` | 0x6f3b3e50 | none | - | - |
//! | `VolumeGroupSetVolume` | 0x6f3b3e30 | none | - | - |
//!
//!

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use jass::vm::{NativeCtx, NativeRegistry, NativeResult, Value};
use softfloat::F32;

use super::game::{Game, HandleKind, LocationObj, ObjKey, Object, PLAYER_SLOTS};
use super::hashtable::{bool_at, int_at, real_at, str_at};

// ---------------------------------------------------------------------------------------------
// Log and events
// ---------------------------------------------------------------------------------------------

/// The kind of a presentation object, as the renderer sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresKind {
    TextTag,
    Sound,
    Effect,
    Lightning,
    Image,
    Ubersplat,
    WeatherEffect,
    TerrainDeformation,
    CameraSetup,
    Quest,
    QuestItem,
    Multiboard,
    MultiboardItem,
    Leaderboard,
    Dialog,
    Button,
    TimerDialog,
}

/// A property value in a [`PresentationEvent::Set`] or [`PresentationEvent::Command`].
#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    Int(i32),
    Real(f32),
    Bool(bool),
    Text(String),
    /// A colour packed as `0xAARRGGBB`.
    Color(u32),
    /// Several reals in order (a point, a velocity, four UVs, ...).
    Reals(Vec<f32>),
    /// A handle value (a unit, a player, ...).
    Handle(u32),
}

/// Where a special effect is placed.
#[derive(Debug, Clone, PartialEq)]
pub enum EffectAt {
    /// `AddSpecialEffect`, `AddSpecialEffectLoc` and the `AddSpellEffect*` forms: a point.
    Point { x: f32, y: f32 },
    /// `AddSpecialEffectTarget`: attached to a widget at an attach point.
    Target { widget: u32, attach: String },
}

/// One thing a renderer or a sound system is asked to show. Events are only appended; the host
/// drains them with [`PresentationLog::drain`].
#[derive(Debug, Clone, PartialEq)]
pub enum PresentationEvent {
    /// A handle object or pool object was created. `handle` is its id (or pool value).
    Created { kind: PresKind, handle: u32 },
    /// The object was destroyed (its handle released, or its pool slot freed).
    Destroyed { kind: PresKind, handle: u32 },
    /// A property of an object was set.
    Set {
        kind: PresKind,
        handle: u32,
        prop: &'static str,
        value: PropValue,
    },
    /// A global setting or a one-shot visual action that is not tied to one object.
    Command {
        name: &'static str,
        args: Vec<PropValue>,
    },

    /// `SetCameraPosition` (`quick` false) and `SetCameraQuickPosition` (`quick` true).
    CameraPosition { x: f32, y: f32, quick: bool },
    /// `PanCameraTo*`. `z` is `None` for the 2D forms, `duration` is 0 for the untimed forms.
    CameraPan {
        x: f32,
        y: f32,
        z: Option<f32>,
        duration: f32,
    },
    /// `ResetToGameCamera`.
    CameraReset { duration: f32 },
    /// `StopCamera`.
    CameraStop,
    /// `CameraSetupApply*`: the setup's fields and destination were applied to the camera. `pan` is
    /// the boolean that asks for a pan (the first boolean of `CameraSetupApply`).
    CameraSetupApplied {
        setup: u32,
        duration: f32,
        pan: bool,
        with_z: bool,
    },
    /// `SetCameraField` (`adjust` false) and `AdjustCameraField` (`adjust` true).
    CameraField {
        field: u32,
        value: f32,
        duration: f32,
        adjust: bool,
    },
    /// `SetCinematicCamera`.
    CinematicCamera { name: String },

    /// A sound was started (`StartSound`).
    SoundStarted { handle: u32 },
    /// A sound was stopped (`StopSound`). `kill` is the `killWhenDone` argument.
    SoundStopped {
        handle: u32,
        fade_out: bool,
        kill: bool,
    },
    /// A sound that was marked with `KillSoundWhenDone` or `StopSound(.., kill)` finished. The
    /// handle is released when this happens (see [`Game::finish_sound`]).
    SoundFinished { handle: u32 },

    /// `PlayMusic`, `PlayMusicEx`, `PlayThematicMusic` and `PlayThematicMusicEx`.
    MusicPlayed {
        name: String,
        thematic: bool,
        start_ms: i32,
        fade_in_ms: i32,
    },
    /// `StopMusic`.
    MusicStopped { fade_out: bool },
    /// `ResumeMusic`.
    MusicResumed,
    /// `SetMapMusic`.
    MapMusicSet {
        name: String,
        random: bool,
        index: i32,
    },
    /// `ClearMapMusic`.
    MapMusicCleared,

    /// `DisplayTextToPlayer` (`duration` None) and `DisplayTimed*` (`duration` set).
    Text {
        player: u32,
        x: f32,
        y: f32,
        duration: Option<f32>,
        text: String,
    },
    /// `ClearTextMessages`.
    TextMessagesCleared,
    /// `SetCinematicScene`.
    CinematicScene {
        portrait: i32,
        speaker: String,
        text: String,
        duration: f32,
        voiceover: f32,
    },
    /// `EndCinematicScene`.
    CinematicSceneEnded,
    /// `AddSpecialEffect*` and `AddSpellEffect*`: the effect to play.
    EffectSpawned {
        handle: u32,
        model: String,
        at: EffectAt,
    },
}

/// The events a renderer or host reads. Events are kept in the order the natives ran.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PresentationLog {
    events: Vec<PresentationEvent>,
}

impl PresentationLog {
    /// Appends an event.
    pub fn push(&mut self, event: PresentationEvent) {
        self.events.push(event);
    }

    /// The events so far, oldest first.
    pub fn events(&self) -> &[PresentationEvent] {
        &self.events
    }

    /// Removes and returns every event so far, oldest first.
    pub fn drain(&mut self) -> Vec<PresentationEvent> {
        std::mem::take(&mut self.events)
    }

    /// Number of events not yet drained.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether no events are waiting.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

// ---------------------------------------------------------------------------------------------
// Pools of non-table handles
// ---------------------------------------------------------------------------------------------

/// A table whose handle value is the slot plus one (0 is null). Freed slots are reused LIFO.
#[derive(Debug, Clone)]
pub struct Pool<T> {
    slots: Vec<Option<T>>,
    free: Vec<u32>,
}

impl<T> Default for Pool<T> {
    fn default() -> Self {
        Pool {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> Pool<T> {
    /// Stores `value` in the most recently freed slot, or in a new slot. Returns its handle value.
    pub fn insert(&mut self, value: T) -> u32 {
        if let Some(i) = self.free.pop() {
            self.slots[i as usize] = Some(value);
            return i + 1;
        }
        self.slots.push(Some(value));
        self.slots.len() as u32
    }

    /// The live entry of handle value `id`.
    pub fn get(&self, id: u32) -> Option<&T> {
        let i = id.checked_sub(1)? as usize;
        self.slots.get(i)?.as_ref()
    }

    /// Mutable access to the live entry of handle value `id`.
    pub fn get_mut(&mut self, id: u32) -> Option<&mut T> {
        let i = id.checked_sub(1)? as usize;
        self.slots.get_mut(i)?.as_mut()
    }

    /// Every live entry, in slot order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.slots.iter_mut().filter_map(|s| s.as_mut())
    }

    /// Frees the entry of handle value `id`. Returns whether there was one.
    pub fn remove(&mut self, id: u32) -> bool {
        let Some(i) = id.checked_sub(1) else {
            return false;
        };
        match self.slots.get_mut(i as usize) {
            Some(slot) if slot.is_some() => {
                *slot = None;
                self.free.push(i);
                true
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Stored state (not handle objects)
// ---------------------------------------------------------------------------------------------

/// Number of camera fields (`CAMERA_FIELD_*`, 0 to 7).
pub const CAMERA_FIELDS: usize = 8;

/// The default camera fields, indexed by `CAMERA_FIELD_*` in this order: target distance, far z,
/// angle of attack, field of view, roll, rotation, z offset, near z. The order follows `common.j`
/// (LIKELY); the values are UNSURE (the game's default camera, not read from the data).
pub const DEFAULT_CAMERA_FIELDS: [f32; CAMERA_FIELDS] =
    [1650.0, 5000.0, 304.0, 70.0, 0.0, 90.0, 0.0, 100.0];

/// A controller of the camera: a unit and an offset from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraController {
    pub unit: u32,
    pub x: f32,
    pub y: f32,
}

/// The camera (one per game; the single local view is not modelled).
#[derive(Debug, Clone, PartialEq)]
pub struct CameraState {
    /// Target position (x, y, z). Set by the position and pan natives.
    pub target: [f32; 3],
    /// Camera fields, indexed by `CAMERA_FIELD_*`.
    pub fields: [f32; CAMERA_FIELDS],
    /// Explicit bounds (min x, min y, max x, max y), or `None` for the playable area.
    pub bounds: Option<[f32; 4]>,
    pub smoothing: f32,
    pub source_noise: (f32, f32),
    pub target_noise: (f32, f32),
    pub target_controller: Option<CameraController>,
    pub orient_controller: Option<CameraController>,
    pub cinematic: String,
}

impl Default for CameraState {
    fn default() -> Self {
        CameraState {
            target: [0.0; 3],
            fields: DEFAULT_CAMERA_FIELDS,
            bounds: None,
            smoothing: 0.0,
            source_noise: (0.0, 0.0),
            target_noise: (0.0, 0.0),
            target_controller: None,
            orient_controller: None,
            cinematic: String::new(),
        }
    }
}

/// The music state. Volume is 0 to 127 (the value the script gave, clamped).
#[derive(Debug, Clone, PartialEq)]
pub struct MusicState {
    pub current: Option<String>,
    pub thematic: bool,
    pub volume: i32,
    pub play_position_ms: i32,
    pub map_music: Option<(String, bool, i32)>,
}

impl Default for MusicState {
    fn default() -> Self {
        MusicState {
            current: None,
            thematic: false,
            volume: 127,
            play_position_ms: 0,
            map_music: None,
        }
    }
}

/// The cinematic filter (`SetCineFilter*`, `DisplayCineFilter`).
#[derive(Debug, Clone, PartialEq)]
pub struct CineFilter {
    pub texture: String,
    pub blend_mode: i32,
    pub tex_map_flags: i32,
    /// Start UVs as (min u, min v, max u, max v), sorted as the original sorts them.
    pub start_uv: [f32; 4],
    pub end_uv: [f32; 4],
    pub start_color: u32,
    pub end_color: u32,
    pub duration: f32,
    pub displayed: bool,
}

impl Default for CineFilter {
    fn default() -> Self {
        CineFilter {
            texture: String::new(),
            blend_mode: 0,
            tex_map_flags: 0,
            start_uv: [0.0, 0.0, 1.0, 1.0],
            end_uv: [0.0, 0.0, 1.0, 1.0],
            start_color: 0xFFFF_FFFF,
            end_color: 0xFFFF_FFFF,
            duration: 0.0,
            displayed: false,
        }
    }
}

/// Interface and user-control switches (`EnableUserControl`, `ShowInterface`, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct UiState {
    pub user_control: bool,
    pub user_ui: bool,
    pub interface_shown: bool,
    pub interface_fade: f32,
    pub cinematic_subtitles: bool,
    pub reserved_hero_buttons: i32,
    pub ally_color_filter: i32,
    pub creep_camp_filter: bool,
    pub minimap_filter: (bool, bool),
    pub multiboard_suppressed: bool,
    pub occlusion: bool,
    pub world_fog_boundary: bool,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            user_control: true,
            user_ui: true,
            interface_shown: true,
            interface_fade: 0.0,
            cinematic_subtitles: false,
            reserved_hero_buttons: 0,
            ally_color_filter: 0,
            creep_camp_filter: false,
            minimap_filter: (false, false),
            multiboard_suppressed: false,
            occlusion: false,
            world_fog_boundary: false,
        }
    }
}

/// The terrain fog (`SetTerrainFogEx`). `SetTerrainFog` itself does nothing in 1.26a (CONFIRMED).
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainFog {
    pub style: i32,
    pub z_start: f32,
    pub z_end: f32,
    pub density: f32,
    pub color: u32,
}

/// Fog, sky and water visuals.
#[derive(Debug, Clone, PartialEq)]
pub struct FogState {
    pub fog_enabled: bool,
    pub mask_enabled: bool,
    pub terrain_fog: Option<TerrainFog>,
    pub sky_model: String,
    pub day_night_models: (String, String),
    pub water_color: u32,
    pub water_deforms: bool,
}

impl Default for FogState {
    fn default() -> Self {
        FogState {
            fog_enabled: true,
            mask_enabled: true,
            terrain_fog: None,
            sky_model: String::new(),
            day_night_models: (String::new(), String::new()),
            water_color: 0,
            water_deforms: true,
        }
    }
}

/// All presentation state of a game. Held by [`Game`]; read it with [`Game::presentation`].
#[derive(Debug, Clone)]
pub struct PresentationState {
    pub log: PresentationLog,
    pub camera: CameraState,
    pub music: MusicState,
    pub cine: CineFilter,
    pub ui: UiState,
    pub fog: FogState,
    /// The volume of the volume groups. 1.26a sets one value for all groups (CONFIRMED: the group
    /// argument of `VolumeGroupSetVolume` is not passed on).
    pub volume_group: f32,
    /// Lengths of sound files in milliseconds, by file name. The host fills it from the game data;
    /// unknown files read as 0.
    pub sound_file_durations: BTreeMap<String, i32>,
    pub text_tags: Pool<TextTag>,
    pub lightnings: Pool<Lightning>,
    pub images: Pool<Image>,
    pub ubersplats: Pool<Ubersplat>,
    pub weather: Pool<Weather>,
    pub deformations: Pool<TerrainDeformation>,
    /// `PlayerSetLeaderboard`: the leaderboard of each player slot.
    pub player_leaderboards: [Option<ObjKey>; PLAYER_SLOTS],
    /// `SetTimeOfDayScale` (UNSURE: the default is the map's, read by the renderer).
    pub time_of_day_scale: f32,
    /// `SetCustomCampaignButtonVisible`, by button index. Unset buttons read as hidden (UNSURE).
    pub custom_campaign_buttons: BTreeMap<i32, bool>,
}

impl Default for PresentationState {
    fn default() -> Self {
        PresentationState {
            log: PresentationLog::default(),
            camera: CameraState::default(),
            music: MusicState::default(),
            cine: CineFilter::default(),
            ui: UiState::default(),
            fog: FogState::default(),
            volume_group: 1.0,
            sound_file_durations: BTreeMap::new(),
            text_tags: Pool::default(),
            lightnings: Pool::default(),
            images: Pool::default(),
            ubersplats: Pool::default(),
            weather: Pool::default(),
            deformations: Pool::default(),
            player_leaderboards: [None; PLAYER_SLOTS],
            time_of_day_scale: 1.0,
            custom_campaign_buttons: BTreeMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Pool objects
// ---------------------------------------------------------------------------------------------

/// A text tag (`CreateTextTag`). Its handle is a pool value (CONFIRMED: the natives index a table
/// with the raw value, they do not resolve a handle).
#[derive(Debug, Clone, PartialEq)]
pub struct TextTag {
    pub text: String,
    /// Text size (`SetTextTagText`'s height).
    pub size: f32,
    pub pos: [f32; 3],
    pub unit: Option<u32>,
    pub color: u32,
    pub velocity: [f32; 2],
    pub visible: bool,
    pub suspended: bool,
    pub permanent: bool,
    pub age: f32,
    pub lifespan: f32,
    pub fadepoint: f32,
}

impl Default for TextTag {
    fn default() -> Self {
        TextTag {
            text: String::new(),
            size: 0.0,
            pos: [0.0; 3],
            unit: None,
            color: 0xFFFF_FFFF,
            velocity: [0.0; 2],
            visible: true,
            suspended: false,
            permanent: false,
            age: 0.0,
            lifespan: 0.0,
            fadepoint: 0.0,
        }
    }
}

/// A lightning bolt. `color` holds the bytes as `[r, g, b, a]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Lightning {
    pub kind: String,
    pub check_visibility: bool,
    /// (x1, y1, z1, x2, y2, z2). The z values are 0 for the 2D forms.
    pub points: [f32; 6],
    pub color: [u8; 4],
}

/// An image (`CreateImage`).
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub file: String,
    pub size: [f32; 3],
    pub position: [f32; 3],
    pub origin: [f32; 3],
    pub image_type: i32,
    pub color: u32,
    pub shown: bool,
    pub render: bool,
    pub render_always: bool,
    pub above_water: bool,
    pub constant_height: Option<f32>,
}

/// An ubersplat (`CreateUbersplat`).
#[derive(Debug, Clone, PartialEq)]
pub struct Ubersplat {
    pub name: String,
    pub position: [f32; 2],
    pub color: u32,
    pub shown: bool,
    pub render: bool,
    pub render_always: bool,
    pub finished: bool,
}

/// A weather effect over a rectangle (`AddWeatherEffect`).
#[derive(Debug, Clone, PartialEq)]
pub struct Weather {
    /// The rectangle as (min x, min y, max x, max y), copied when the effect was added.
    pub rect: [f32; 4],
    pub effect_id: i32,
    pub enabled: bool,
}

/// A terrain deformation (`TerrainDeform*`).
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainDeformation {
    pub kind: &'static str,
    pub params: Vec<f32>,
    pub duration: f32,
    pub stopped: bool,
}

// ---------------------------------------------------------------------------------------------
// Handle objects (handle table)
// ---------------------------------------------------------------------------------------------

/// Where a sound came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundSource {
    File,
    Label,
    Midi,
}

/// A sound (`CreateSound*`, `CreateMIDISound`).
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    pub source: SoundSource,
    /// The file name, or the label for `CreateSoundFromLabel` and `CreateMIDISound`.
    pub name: String,
    pub looping: bool,
    pub is_3d: bool,
    pub stop_out_of_range: bool,
    pub fade_in: i32,
    pub fade_out: i32,
    /// The EAX setting name (`CreateSound` only).
    pub eax: String,
    pub channel: i32,
    /// 0 to 127.
    pub volume: i32,
    pub pitch: f32,
    pub play_position_ms: i32,
    /// `SetSoundDuration` value, or the file's length when the sound is created (0 if unknown).
    pub duration_ms: i32,
    pub label: String,
    pub min_distance: f32,
    pub max_distance: f32,
    pub distance_cutoff: f32,
    pub cone: (f32, f32, i32),
    pub cone_orientation: [f32; 3],
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub attached_unit: Option<u32>,
    pub playing: bool,
    pub started: bool,
    pub kill_when_done: bool,
}

/// An effect (`AddSpecialEffect*`, `AddSpellEffect*`). For the spell forms `model` is the ability
/// name, or `#<id>` for the raw id, and `effect_type` is the `EFFECT_TYPE_*` value.
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    pub model: String,
    pub at: EffectAt,
    pub effect_type: Option<u32>,
}

/// A camera setup (`CreateCameraSetup`). Its handle is from the handle table with flag 0.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraSetup {
    pub fields: [f32; CAMERA_FIELDS],
    pub dest: [f32; 3],
}

/// A quest (`CreateQuest`). `flags` bits: 1 completed, 2 required, 4 discovered, 8 failed,
/// 0x10 enabled (the bit layout is read from the natives).
#[derive(Debug, Clone, PartialEq)]
pub struct Quest {
    pub title: String,
    pub description: String,
    pub icon: String,
    pub flags: u32,
}

/// A quest item (`QuestCreateItem`).
#[derive(Debug, Clone, PartialEq)]
pub struct QuestItem {
    pub quest: ObjKey,
    pub description: String,
    pub completed: bool,
}

/// Defaults that `MultiboardSetItems*` writes for all items of a board.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemDefaults {
    pub show_value: bool,
    pub show_icon: bool,
    pub value: String,
    pub value_color: u32,
    pub width: f32,
    pub icon: String,
}

impl Default for ItemDefaults {
    fn default() -> Self {
        ItemDefaults {
            show_value: true,
            show_icon: false,
            value: String::new(),
            value_color: 0xFFFF_FFFF,
            width: 0.0,
            icon: String::new(),
        }
    }
}

/// A multiboard (`CreateMultiboard`). Rows and columns are the counts the script set (+0x48, +0x4c).
#[derive(Debug, Clone, PartialEq)]
pub struct Multiboard {
    pub title: String,
    pub title_color: u32,
    pub rows: u32,
    pub cols: u32,
    pub displayed: bool,
    pub minimized: bool,
    pub defaults: ItemDefaults,
    /// The item objects of the cells that were asked for (row, col) -> key.
    pub cells: BTreeMap<(u32, u32), ObjKey>,
}

impl Default for Multiboard {
    fn default() -> Self {
        Multiboard {
            title: String::new(),
            title_color: 0xFFFF_FFFF,
            rows: 0,
            cols: 0,
            displayed: false,
            minimized: false,
            defaults: ItemDefaults::default(),
            cells: BTreeMap::new(),
        }
    }
}

/// One cell of a multiboard (`MultiboardGetItem`). Settings that were given for the item itself
/// are kept in `own`; the board's defaults apply otherwise.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiboardItem {
    pub board: ObjKey,
    pub row: u32,
    pub col: u32,
    pub value: Option<String>,
    pub value_color: Option<u32>,
    pub width: Option<f32>,
    pub icon: Option<String>,
    pub style: Option<(bool, bool)>,
}

/// One row of a leaderboard.
#[derive(Debug, Clone, PartialEq)]
pub struct LeaderboardItem {
    /// The player slot, or `None` for an item without a player.
    pub player: Option<u32>,
    pub label: String,
    pub value: i32,
    pub label_color: u32,
    pub value_color: u32,
    pub show_label: bool,
    pub show_value: bool,
    pub show_icon: bool,
}

/// A leaderboard (`CreateLeaderboard`). `style` is (show label, show names, show values, show icons).
#[derive(Debug, Clone, PartialEq)]
pub struct Leaderboard {
    pub label: String,
    pub label_color: u32,
    pub value_color: u32,
    pub style: (bool, bool, bool, bool),
    pub size: i32,
    pub displayed: bool,
    pub items: Vec<LeaderboardItem>,
}

impl Default for Leaderboard {
    fn default() -> Self {
        Leaderboard {
            label: String::new(),
            label_color: 0xFFFF_FFFF,
            value_color: 0xFFFF_FFFF,
            style: (true, true, true, true),
            size: 0,
            displayed: false,
            items: Vec::new(),
        }
    }
}

/// A dialog (`DialogCreate`). `shown_for` is a bit mask of player slots (bit `i` = `Player(i)`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dialog {
    pub message: String,
    pub buttons: Vec<ObjKey>,
    pub is_async: bool,
    pub shown_for: u32,
}

/// A dialog button (`DialogAddButton`, `DialogAddQuitButton`).
#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    pub dialog: ObjKey,
    pub text: String,
    pub hotkey: i32,
    pub quit: bool,
    pub score_screen: bool,
}

/// A timer dialog (`CreateTimerDialog`). `timer` is the timer it shows, if the handle was valid.
#[derive(Debug, Clone, PartialEq)]
pub struct TimerDialog {
    pub timer: Option<ObjKey>,
    pub title: String,
    pub title_color: u32,
    pub time_color: u32,
    pub speed: f32,
    pub real_remaining: f32,
    pub displayed: bool,
}

impl Default for TimerDialog {
    fn default() -> Self {
        TimerDialog {
            timer: None,
            title: String::new(),
            title_color: 0xFFFF_FFFF,
            time_color: 0xFFFF_FFFF,
            speed: 1.0,
            real_remaining: 0.0,
            displayed: false,
        }
    }
}

/// A presentation object stored in the game's arena (`Object::Pres`).
#[derive(Debug, Clone, PartialEq)]
pub enum PresObj {
    CameraSetup(CameraSetup),
    Sound(Sound),
    Effect(Effect),
    Quest(Quest),
    QuestItem(QuestItem),
    Multiboard(Multiboard),
    MultiboardItem(MultiboardItem),
    Leaderboard(Leaderboard),
    Dialog(Dialog),
    Button(Button),
    TimerDialog(TimerDialog),
}

impl PresObj {
    /// The handle type of this object.
    pub fn handle_kind(&self) -> HandleKind {
        match self {
            PresObj::CameraSetup(_) => HandleKind::CameraSetup,
            PresObj::Sound(_) => HandleKind::Sound,
            PresObj::Effect(_) => HandleKind::Effect,
            PresObj::Quest(_) => HandleKind::Quest,
            PresObj::QuestItem(_) => HandleKind::QuestItem,
            PresObj::Multiboard(_) => HandleKind::Multiboard,
            PresObj::MultiboardItem(_) => HandleKind::MultiboardItem,
            PresObj::Leaderboard(_) => HandleKind::Leaderboard,
            PresObj::Dialog(_) => HandleKind::Dialog,
            PresObj::Button(_) => HandleKind::Button,
            PresObj::TimerDialog(_) => HandleKind::TimerDialog,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

fn r(a: &[Value], i: usize) -> f32 {
    real_at(a, i).to_f32()
}

fn n(a: &[Value], i: usize) -> i32 {
    int_at(a, i)
}

fn flag(a: &[Value], i: usize) -> bool {
    bool_at(a, i)
}

/// A handle or a raw integer argument (player colours and some unit arguments are raw values).
fn h(a: &[Value], i: usize) -> u32 {
    a.get(i).and_then(|v| v.as_handle()).unwrap_or(0)
}

fn raw(a: &[Value], i: usize) -> u32 {
    match a.get(i) {
        Some(Value::Handle(x)) => *x,
        Some(Value::Integer(x)) => *x as u32,
        _ => 0,
    }
}

fn vr(x: f32) -> Value {
    Value::Real(F32::from_bits(x.to_bits()))
}

fn vs(ctx: &mut NativeCtx<'_>, s: &str) -> Value {
    Value::String(ctx.intern(s))
}

/// Packs four integer arguments (r, g, b, a) as `0xAARRGGBB`.
fn argb(a: &[Value], i: usize) -> u32 {
    let c = |k: usize| (n(a, i + k) as u32) & 0xff;
    (c(3) << 24) | (c(0) << 16) | (c(1) << 8) | c(2)
}

/// Packs four real arguments (r, g, b, a in 0..1) as bytes `[r, g, b, a]`.
fn rgba_bytes(a: &[Value], i: usize) -> [u8; 4] {
    let c = |k: usize| ((r(a, i + k) * 255.0).round() as i32 & 0xff) as u8;
    [c(0), c(1), c(2), c(3)]
}

fn argb_of(c: [u8; 4]) -> u32 {
    ((c[3] as u32) << 24) | ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32
}

fn null() -> Value {
    Value::Handle(0)
}

fn log(gm: &mut Game, e: PresentationEvent) {
    gm.presentation_mut().log.push(e);
}

fn create(gm: &mut Game, obj: PresObj, flag: u32) -> u32 {
    gm.add_object(Object::Pres(obj), flag).1
}

/// A location handle with `x` and `y` (flag 1, as the `Location` native gives it).
fn new_location(gm: &mut Game, x: f32, y: f32) -> u32 {
    let loc = LocationObj {
        x: F32::from_bits(x.to_bits()),
        y: F32::from_bits(y.to_bits()),
    };
    gm.add_object(Object::Location(loc), 1).1
}

/// The point of a live location handle, or `None`.
fn location_xy(gm: &Game, id: u32) -> Option<(f32, f32)> {
    let k = gm.handle_of_kind(id, HandleKind::Location)?;
    match gm.object(k) {
        Object::Location(l) => Some((l.x.to_f32(), l.y.to_f32())),
        _ => None,
    }
}

/// Releases a handle object of `kind` (its death call) and logs it. Null, freed and wrong-kind
/// handles do nothing.
fn destroy(gm: &mut Game, id: u32, kind: HandleKind, pk: PresKind) {
    if let Some(k) = gm.handle_of_kind(id, kind) {
        gm.kill(k);
        log(
            gm,
            PresentationEvent::Destroyed {
                kind: pk,
                handle: id,
            },
        );
    }
}

fn set(gm: &mut Game, kind: PresKind, handle: u32, prop: &'static str, value: PropValue) {
    log(
        gm,
        PresentationEvent::Set {
            kind,
            handle,
            prop,
            value,
        },
    );
}

fn command(gm: &mut Game, name: &'static str, args: Vec<PropValue>) {
    log(gm, PresentationEvent::Command { name, args });
}

/// Defines the accessor `$fn(gm, id) -> Option<&mut $ty>` for a handle object of `$kind`.
macro_rules! accessor {
    ($fn:ident, $kind:ident, $var:ident, $ty:ty) => {
        fn $fn(gm: &mut Game, id: u32) -> Option<&mut $ty> {
            let k = gm.handle_of_kind(id, HandleKind::$kind)?;
            match gm.object_mut(k) {
                Object::Pres(PresObj::$var(o)) => Some(o),
                _ => None,
            }
        }
    };
}

accessor!(sound_mut, Sound, Sound, Sound);
accessor!(camera_setup_mut, CameraSetup, CameraSetup, CameraSetup);
accessor!(quest_mut, Quest, Quest, Quest);
accessor!(quest_item_mut, QuestItem, QuestItem, QuestItem);
accessor!(multiboard_mut, Multiboard, Multiboard, Multiboard);
accessor!(
    multiboard_item_mut,
    MultiboardItem,
    MultiboardItem,
    MultiboardItem
);
accessor!(leaderboard_mut, Leaderboard, Leaderboard, Leaderboard);
accessor!(dialog_mut, Dialog, Dialog, Dialog);
accessor!(timer_dialog_mut, TimerDialog, TimerDialog, TimerDialog);

/// The four edges of a live rect handle, or `None`.
fn rect_edges(gm: &Game, id: u32) -> Option<[f32; 4]> {
    let k = gm.handle_of_kind(id, HandleKind::Rect)?;
    match gm.object(k) {
        Object::Rect(rc) => Some([
            rc.min_x.to_f32(),
            rc.min_y.to_f32(),
            rc.max_x.to_f32(),
            rc.max_y.to_f32(),
        ]),
        _ => None,
    }
}

/// The player slot of a live player handle, or `None`.
fn player_slot(gm: &Game, id: u32) -> Option<u32> {
    gm.player_index_of_handle(id)
}

fn min_max(vals: &[f32]) -> (f32, f32) {
    let mut lo = vals[0];
    let mut hi = vals[0];
    for &v in &vals[1..] {
        if v < lo {
            lo = v;
        }
        if v > hi {
            hi = v;
        }
    }
    (lo, hi)
}

/// The sorted UV pair of the cinematic filter natives: (min u, min v, max u, max v).
fn uv_sorted(a: &[Value]) -> [f32; 4] {
    let (x1, y1, x2, y2) = (r(a, 0), r(a, 1), r(a, 2), r(a, 3));
    let (u0, u1) = min_max(&[x1, x2]);
    let (v0, v1) = min_max(&[y1, y2]);
    [u0, v0, u1, v1]
}

impl Game {
    /// The sound `handle` finished playing. A sound marked with `KillSoundWhenDone` (or stopped
    /// with `killWhenDone`) releases its handle now; any other sound only stops. The host calls this
    /// when its audio ends, because playback is not timed here. Returns whether the handle was freed.
    pub fn finish_sound(&mut self, handle: u32) -> bool {
        let Some(k) = self.handle_of_kind(handle, HandleKind::Sound) else {
            return false;
        };
        let kill = match self.object_mut(k) {
            Object::Pres(PresObj::Sound(s)) => {
                s.playing = false;
                s.kill_when_done
            }
            _ => false,
        };
        log(self, PresentationEvent::SoundFinished { handle });
        if kill {
            destroy(self, handle, HandleKind::Sound, PresKind::Sound);
        }
        kill
    }
}

// ---------------------------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------------------------

/// Registers one native: `native!(reg, game, "Name", |gm, ctx, a| { ...; value })`. `gm` is the
/// locked game, `ctx` the native context, `a` the arguments; the block gives the returned value.
macro_rules! native {
    ($reg:ident, $game:ident, $name:expr, |$gm:ident, $ctx:ident, $a:ident| $body:expr) => {{
        let g = Rc::clone($game);
        $reg.register($name, move |$ctx: &mut NativeCtx<'_>, $a: &[Value]| {
            #[allow(unused_mut)]
            let mut $gm = g.borrow_mut();
            // The body is a closure so that it can `return` a value early.
            #[allow(clippy::redundant_closure_call)]
            let value: Value = (|| $body)();
            NativeResult::Return(value)
        });
    }};
}

/// Registers the presentation natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    register_camera(reg, game);
    register_sound(reg, game);
    register_text(reg, game);
    register_objects(reg, game);
    register_visuals(reg, game);
    register_pools(reg, game);
    register_misc(reg, game);
}

// ---------------------------------------------------------------------------------------------
// Camera
// ---------------------------------------------------------------------------------------------

/// The camera bounds: the explicit ones, else the playable area, else zeros.
fn camera_bounds(gm: &Game) -> [f32; 4] {
    if let Some(b) = gm.presentation().camera.bounds {
        return b;
    }
    match gm.playable() {
        Some(p) => [
            p.min_x.to_f32(),
            p.min_y.to_f32(),
            p.max_x.to_f32(),
            p.max_y.to_f32(),
        ],
        None => [0.0; 4],
    }
}

/// Applies the camera setup `id` to the camera: its fields, and its destination as the target when
/// `pan` is set. `z` (when given) sets the target height. Logs the application.
fn apply_camera_setup(
    gm: &mut Game,
    id: u32,
    duration: f32,
    pan: bool,
    with_z: bool,
    z: Option<f32>,
) {
    let Some((fields, dest)) = camera_setup_mut(gm, id).map(|s| (s.fields, s.dest)) else {
        return;
    };
    let cam = &mut gm.presentation_mut().camera;
    cam.fields = fields;
    if pan {
        cam.target[0] = dest[0];
        cam.target[1] = dest[1];
    }
    if let Some(z) = z {
        cam.target[2] = z;
    }
    log(
        gm,
        PresentationEvent::CameraSetupApplied {
            setup: id,
            duration,
            pan,
            with_z,
        },
    );
}

fn register_camera(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    native!(reg, game, "SetCameraBounds", |gm, _ctx, a| {
        let (min_x, max_x) = min_max(&[r(a, 0), r(a, 2), r(a, 4), r(a, 6)]);
        let (min_y, max_y) = min_max(&[r(a, 1), r(a, 3), r(a, 5), r(a, 7)]);
        let bounds = [min_x, min_y, max_x, max_y];
        gm.presentation_mut().camera.bounds = Some(bounds);
        command(
            &mut gm,
            "CameraBounds",
            bounds.iter().map(|&v| PropValue::Real(v)).collect(),
        );
        Value::Null
    });

    native!(reg, game, "SetCameraPosition", |gm, _ctx, a| {
        let (x, y) = (r(a, 0), r(a, 1));
        let cam = &mut gm.presentation_mut().camera;
        cam.target[0] = x;
        cam.target[1] = y;
        log(
            &mut gm,
            PresentationEvent::CameraPosition { x, y, quick: false },
        );
        Value::Null
    });

    native!(reg, game, "SetCameraQuickPosition", |gm, _ctx, a| {
        let (x, y) = (r(a, 0), r(a, 1));
        let cam = &mut gm.presentation_mut().camera;
        cam.target[0] = x;
        cam.target[1] = y;
        log(
            &mut gm,
            PresentationEvent::CameraPosition { x, y, quick: true },
        );
        Value::Null
    });

    native!(reg, game, "ResetToGameCamera", |gm, _ctx, a| {
        let duration = r(a, 0);
        let cam = &mut gm.presentation_mut().camera;
        cam.fields = DEFAULT_CAMERA_FIELDS;
        cam.target_controller = None;
        cam.orient_controller = None;
        log(&mut gm, PresentationEvent::CameraReset { duration });
        Value::Null
    });

    native!(reg, game, "StopCamera", |gm, _ctx, _a| {
        let cam = &mut gm.presentation_mut().camera;
        cam.target_controller = None;
        cam.orient_controller = None;
        log(&mut gm, PresentationEvent::CameraStop);
        Value::Null
    });

    native!(reg, game, "PanCameraTo", |gm, _ctx, a| {
        let (x, y) = (r(a, 0), r(a, 1));
        let cam = &mut gm.presentation_mut().camera;
        cam.target[0] = x;
        cam.target[1] = y;
        log(
            &mut gm,
            PresentationEvent::CameraPan {
                x,
                y,
                z: None,
                duration: 0.0,
            },
        );
        Value::Null
    });

    native!(reg, game, "PanCameraToTimed", |gm, _ctx, a| {
        let (x, y, duration) = (r(a, 0), r(a, 1), r(a, 2));
        let cam = &mut gm.presentation_mut().camera;
        cam.target[0] = x;
        cam.target[1] = y;
        log(
            &mut gm,
            PresentationEvent::CameraPan {
                x,
                y,
                z: None,
                duration,
            },
        );
        Value::Null
    });

    native!(reg, game, "PanCameraToWithZ", |gm, _ctx, a| {
        let (x, y, z) = (r(a, 0), r(a, 1), r(a, 2));
        let cam = &mut gm.presentation_mut().camera;
        cam.target = [x, y, z];
        log(
            &mut gm,
            PresentationEvent::CameraPan {
                x,
                y,
                z: Some(z),
                duration: 0.0,
            },
        );
        Value::Null
    });

    native!(reg, game, "PanCameraToTimedWithZ", |gm, _ctx, a| {
        let (x, y, z, duration) = (r(a, 0), r(a, 1), r(a, 2), r(a, 3));
        let cam = &mut gm.presentation_mut().camera;
        cam.target = [x, y, z];
        log(
            &mut gm,
            PresentationEvent::CameraPan {
                x,
                y,
                z: Some(z),
                duration,
            },
        );
        Value::Null
    });

    native!(reg, game, "SetCinematicCamera", |gm, ctx, a| {
        let name = str_at(ctx, a, 0);
        gm.presentation_mut().camera.cinematic = name.clone();
        log(&mut gm, PresentationEvent::CinematicCamera { name });
        Value::Null
    });

    native!(reg, game, "SetCameraField", |gm, _ctx, a| {
        let field = raw(a, 0) as usize;
        let (value, duration) = (r(a, 1), r(a, 2));
        if field < CAMERA_FIELDS {
            gm.presentation_mut().camera.fields[field] = value;
        }
        log(
            &mut gm,
            PresentationEvent::CameraField {
                field: field as u32,
                value,
                duration,
                adjust: false,
            },
        );
        Value::Null
    });

    native!(reg, game, "AdjustCameraField", |gm, _ctx, a| {
        let field = raw(a, 0) as usize;
        let (offset, duration) = (r(a, 1), r(a, 2));
        if field < CAMERA_FIELDS {
            gm.presentation_mut().camera.fields[field] += offset;
        }
        log(
            &mut gm,
            PresentationEvent::CameraField {
                field: field as u32,
                value: offset,
                duration,
                adjust: true,
            },
        );
        Value::Null
    });

    native!(reg, game, "SetCameraTargetController", |gm, _ctx, a| {
        let unit = raw(a, 0);
        let (x, y) = (r(a, 1), r(a, 2));
        gm.presentation_mut().camera.target_controller = Some(CameraController { unit, x, y });
        command(
            &mut gm,
            "CameraTargetController",
            vec![
                PropValue::Handle(unit),
                PropValue::Real(x),
                PropValue::Real(y),
                PropValue::Bool(flag(a, 3)),
            ],
        );
        Value::Null
    });

    native!(reg, game, "SetCameraOrientController", |gm, _ctx, a| {
        let unit = raw(a, 0);
        let (x, y) = (r(a, 1), r(a, 2));
        gm.presentation_mut().camera.orient_controller = Some(CameraController { unit, x, y });
        command(
            &mut gm,
            "CameraOrientController",
            vec![
                PropValue::Handle(unit),
                PropValue::Real(x),
                PropValue::Real(y),
            ],
        );
        Value::Null
    });

    native!(reg, game, "SetCameraRotateMode", |gm, _ctx, a| {
        let args = (0..4).map(|i| PropValue::Real(r(a, i))).collect();
        command(&mut gm, "CameraRotateMode", args);
        Value::Null
    });

    native!(reg, game, "CreateCameraSetup", |gm, _ctx, _a| {
        let obj = CameraSetup {
            fields: DEFAULT_CAMERA_FIELDS,
            dest: [0.0; 3],
        };
        let id = create(&mut gm, PresObj::CameraSetup(obj), 0);
        log(
            &mut gm,
            PresentationEvent::Created {
                kind: PresKind::CameraSetup,
                handle: id,
            },
        );
        Value::Handle(id)
    });

    native!(reg, game, "CameraSetupSetField", |gm, _ctx, a| {
        let field = raw(a, 1) as usize;
        let value = r(a, 2);
        if let Some(s) = camera_setup_mut(&mut gm, h(a, 0)) {
            if field < CAMERA_FIELDS {
                s.fields[field] = value;
            }
        }
        Value::Null
    });

    native!(reg, game, "CameraSetupGetField", |gm, _ctx, a| {
        let field = raw(a, 1) as usize;
        let v = match camera_setup_mut(&mut gm, h(a, 0)) {
            Some(s) if field < CAMERA_FIELDS => s.fields[field],
            _ => 0.0,
        };
        vr(v)
    });

    native!(reg, game, "CameraSetupSetDestPosition", |gm, _ctx, a| {
        let (x, y) = (r(a, 1), r(a, 2));
        if let Some(s) = camera_setup_mut(&mut gm, h(a, 0)) {
            s.dest = [x, y, 0.0];
        }
        Value::Null
    });

    native!(reg, game, "CameraSetupGetDestPositionLoc", |gm, _ctx, a| {
        match camera_setup_mut(&mut gm, h(a, 0)).map(|s| (s.dest[0], s.dest[1])) {
            Some((x, y)) => Value::Handle(new_location(&mut gm, x, y)),
            None => null(),
        }
    });

    native!(reg, game, "CameraSetupGetDestPositionX", |gm, _ctx, a| {
        vr(camera_setup_mut(&mut gm, h(a, 0)).map_or(0.0, |s| s.dest[0]))
    });

    native!(reg, game, "CameraSetupGetDestPositionY", |gm, _ctx, a| {
        vr(camera_setup_mut(&mut gm, h(a, 0)).map_or(0.0, |s| s.dest[1]))
    });

    native!(reg, game, "CameraSetupApply", |gm, _ctx, a| {
        apply_camera_setup(&mut gm, h(a, 0), 0.0, flag(a, 1), false, None);
        Value::Null
    });

    native!(reg, game, "CameraSetupApplyWithZ", |gm, _ctx, a| {
        apply_camera_setup(&mut gm, h(a, 0), 0.0, true, true, Some(r(a, 1)));
        Value::Null
    });

    native!(reg, game, "CameraSetupApplyForceDuration", |gm, _ctx, a| {
        apply_camera_setup(&mut gm, h(a, 0), r(a, 2), flag(a, 1), false, None);
        Value::Null
    });

    native!(
        reg,
        game,
        "CameraSetupApplyForceDurationWithZ",
        |gm, _ctx, a| {
            apply_camera_setup(&mut gm, h(a, 0), r(a, 2), true, true, Some(r(a, 1)));
            Value::Null
        }
    );

    // Noise values are stored as given. No getter reads them, so the original's thresholds do not
    // matter for the model (UNSURE what the renderer does with them).
    native!(reg, game, "CameraSetTargetNoise", |gm, _ctx, a| {
        let v = (r(a, 0), r(a, 1));
        gm.presentation_mut().camera.target_noise = v;
        command(
            &mut gm,
            "CameraTargetNoise",
            vec![PropValue::Reals(vec![v.0, v.1])],
        );
        Value::Null
    });

    native!(reg, game, "CameraSetSourceNoise", |gm, _ctx, a| {
        let v = (r(a, 0), r(a, 1));
        gm.presentation_mut().camera.source_noise = v;
        command(
            &mut gm,
            "CameraSourceNoise",
            vec![PropValue::Reals(vec![v.0, v.1])],
        );
        Value::Null
    });

    native!(reg, game, "CameraSetTargetNoiseEx", |gm, _ctx, a| {
        let v = (r(a, 0), r(a, 1));
        gm.presentation_mut().camera.target_noise = v;
        command(
            &mut gm,
            "CameraTargetNoiseEx",
            vec![
                PropValue::Reals(vec![v.0, v.1]),
                PropValue::Bool(flag(a, 2)),
            ],
        );
        Value::Null
    });

    native!(reg, game, "CameraSetSourceNoiseEx", |gm, _ctx, a| {
        let v = (r(a, 0), r(a, 1));
        gm.presentation_mut().camera.source_noise = v;
        command(
            &mut gm,
            "CameraSourceNoiseEx",
            vec![
                PropValue::Reals(vec![v.0, v.1]),
                PropValue::Bool(flag(a, 2)),
            ],
        );
        Value::Null
    });

    native!(reg, game, "CameraSetSmoothingFactor", |gm, _ctx, a| {
        let f = r(a, 0);
        gm.presentation_mut().camera.smoothing = f;
        command(&mut gm, "CameraSmoothingFactor", vec![PropValue::Real(f)]);
        Value::Null
    });

    // The margins depend on the screen aspect ratio, which the renderer owns (UNSURE): 0.
    native!(reg, game, "GetCameraMargin", |_gm, _ctx, _a| { vr(0.0) });

    native!(reg, game, "GetCameraBoundMinX", |gm, _ctx, _a| {
        vr(camera_bounds(&gm)[0])
    });
    native!(reg, game, "GetCameraBoundMinY", |gm, _ctx, _a| {
        vr(camera_bounds(&gm)[1])
    });
    native!(reg, game, "GetCameraBoundMaxX", |gm, _ctx, _a| {
        vr(camera_bounds(&gm)[2])
    });
    native!(reg, game, "GetCameraBoundMaxY", |gm, _ctx, _a| {
        vr(camera_bounds(&gm)[3])
    });

    native!(reg, game, "GetCameraField", |gm, _ctx, a| {
        let field = raw(a, 0) as usize;
        vr(gm
            .presentation()
            .camera
            .fields
            .get(field)
            .copied()
            .unwrap_or(0.0))
    });

    native!(reg, game, "GetCameraTargetPositionX", |gm, _ctx, _a| {
        vr(gm.presentation().camera.target[0])
    });
    native!(reg, game, "GetCameraTargetPositionY", |gm, _ctx, _a| {
        vr(gm.presentation().camera.target[1])
    });
    native!(reg, game, "GetCameraTargetPositionZ", |gm, _ctx, _a| {
        vr(gm.presentation().camera.target[2])
    });

    native!(reg, game, "GetCameraTargetPositionLoc", |gm, _ctx, _a| {
        let t = gm.presentation().camera.target;
        Value::Handle(new_location(&mut gm, t[0], t[1]))
    });

    // The eye position is the target until the renderer computes the view (UNSURE).
    native!(reg, game, "GetCameraEyePositionX", |gm, _ctx, _a| {
        vr(gm.presentation().camera.target[0])
    });
    native!(reg, game, "GetCameraEyePositionY", |gm, _ctx, _a| {
        vr(gm.presentation().camera.target[1])
    });
    native!(reg, game, "GetCameraEyePositionZ", |gm, _ctx, _a| {
        vr(gm.presentation().camera.target[2])
    });

    native!(reg, game, "GetCameraEyePositionLoc", |gm, _ctx, _a| {
        let t = gm.presentation().camera.target;
        Value::Handle(new_location(&mut gm, t[0], t[1]))
    });
}

// ---------------------------------------------------------------------------------------------
// Sound and music
// ---------------------------------------------------------------------------------------------

/// Creates a sound with the given source and flags, and returns its handle (flag 1, as the
/// original's `CreateSound` passes to the handle allocator).
#[allow(clippy::too_many_arguments)]
fn new_sound(
    gm: &mut Game,
    source: SoundSource,
    name: String,
    looping: bool,
    is_3d: bool,
    stop_out_of_range: bool,
    fades: (i32, i32),
    eax: String,
) -> u32 {
    let duration_ms = match source {
        SoundSource::File => gm
            .presentation()
            .sound_file_durations
            .get(&name)
            .copied()
            .unwrap_or(0),
        _ => 0,
    };
    let sound = Sound {
        source,
        name,
        looping,
        is_3d,
        stop_out_of_range,
        fade_in: fades.0,
        fade_out: fades.1,
        eax,
        channel: 0,
        volume: 127,
        pitch: 1.0,
        play_position_ms: 0,
        duration_ms,
        label: String::new(),
        min_distance: 0.0,
        max_distance: 0.0,
        distance_cutoff: 0.0,
        cone: (0.0, 0.0, 0),
        cone_orientation: [0.0; 3],
        position: [0.0; 3],
        velocity: [0.0; 3],
        attached_unit: None,
        playing: false,
        started: false,
        kill_when_done: false,
    };
    let id = create(gm, PresObj::Sound(sound), 1);
    log(
        gm,
        PresentationEvent::Created {
            kind: PresKind::Sound,
            handle: id,
        },
    );
    id
}

/// Sets a sound property and logs it, when the handle is a live sound.
fn sound_set(
    gm: &mut Game,
    id: u32,
    prop: &'static str,
    value: PropValue,
    f: impl FnOnce(&mut Sound),
) {
    if let Some(s) = sound_mut(gm, id) {
        f(s);
        set(gm, PresKind::Sound, id, prop, value);
    }
}

/// Starts a music track (`PlayMusic*` and `PlayThematicMusic*`).
fn play_music(gm: &mut Game, name: String, thematic: bool, start_ms: i32, fade_in_ms: i32) {
    let m = &mut gm.presentation_mut().music;
    m.current = Some(name.clone());
    m.thematic = thematic;
    m.play_position_ms = start_ms;
    log(
        gm,
        PresentationEvent::MusicPlayed {
            name,
            thematic,
            start_ms,
            fade_in_ms,
        },
    );
}

fn register_sound(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    native!(reg, game, "CreateSound", |gm, ctx, a| {
        let file = str_at(ctx, a, 0);
        let eax = str_at(ctx, a, 6);
        let fades = (n(a, 4), n(a, 5));
        Value::Handle(new_sound(
            &mut gm,
            SoundSource::File,
            file,
            flag(a, 1),
            flag(a, 2),
            flag(a, 3),
            fades,
            eax,
        ))
    });

    native!(reg, game, "CreateSoundFilenameWithLabel", |gm, ctx, a| {
        let file = str_at(ctx, a, 0);
        let label = str_at(ctx, a, 6);
        let fades = (n(a, 4), n(a, 5));
        let id = new_sound(
            &mut gm,
            SoundSource::File,
            file,
            flag(a, 1),
            flag(a, 2),
            flag(a, 3),
            fades,
            String::new(),
        );
        sound_set(&mut gm, id, "label", PropValue::Text(label.clone()), |s| {
            s.label = label
        });
        Value::Handle(id)
    });

    native!(reg, game, "CreateSoundFromLabel", |gm, ctx, a| {
        let label = str_at(ctx, a, 0);
        let fades = (n(a, 4), n(a, 5));
        Value::Handle(new_sound(
            &mut gm,
            SoundSource::Label,
            label,
            flag(a, 1),
            flag(a, 2),
            flag(a, 3),
            fades,
            String::new(),
        ))
    });

    native!(reg, game, "CreateMIDISound", |gm, ctx, a| {
        let label = str_at(ctx, a, 0);
        let fades = (n(a, 1), n(a, 2));
        Value::Handle(new_sound(
            &mut gm,
            SoundSource::Midi,
            label,
            false,
            false,
            false,
            fades,
            String::new(),
        ))
    });

    native!(reg, game, "SetStackedSound", |gm, ctx, a| {
        let name = str_at(ctx, a, 0);
        command(
            &mut gm,
            "SetStackedSound",
            vec![
                PropValue::Text(name),
                PropValue::Real(r(a, 1)),
                PropValue::Real(r(a, 2)),
            ],
        );
        Value::Null
    });

    native!(reg, game, "ClearStackedSound", |gm, ctx, a| {
        let name = str_at(ctx, a, 0);
        command(
            &mut gm,
            "ClearStackedSound",
            vec![
                PropValue::Text(name),
                PropValue::Real(r(a, 1)),
                PropValue::Real(r(a, 2)),
            ],
        );
        Value::Null
    });

    native!(reg, game, "SetStackedSoundRect", |gm, ctx, a| {
        let name = str_at(ctx, a, 0);
        if let Some(e) = rect_edges(&gm, h(a, 1)) {
            command(
                &mut gm,
                "SetStackedSoundRect",
                vec![PropValue::Text(name), PropValue::Reals(e.to_vec())],
            );
        }
        Value::Null
    });

    native!(reg, game, "ClearStackedSoundRect", |gm, ctx, a| {
        let name = str_at(ctx, a, 0);
        if let Some(e) = rect_edges(&gm, h(a, 1)) {
            command(
                &mut gm,
                "ClearStackedSoundRect",
                vec![PropValue::Text(name), PropValue::Reals(e.to_vec())],
            );
        }
        Value::Null
    });

    native!(reg, game, "RegisterStackedSound", |gm, _ctx, a| {
        let s = h(a, 0);
        if sound_mut(&mut gm, s).is_some() {
            let args = vec![
                PropValue::Handle(s),
                PropValue::Bool(flag(a, 1)),
                PropValue::Reals(vec![r(a, 2), r(a, 3)]),
            ];
            command(&mut gm, "RegisterStackedSound", args);
        }
        Value::Null
    });

    native!(reg, game, "UnregisterStackedSound", |gm, _ctx, a| {
        let s = h(a, 0);
        if sound_mut(&mut gm, s).is_some() {
            let args = vec![
                PropValue::Handle(s),
                PropValue::Bool(flag(a, 1)),
                PropValue::Reals(vec![r(a, 2), r(a, 3)]),
            ];
            command(&mut gm, "UnregisterStackedSound", args);
        }
        Value::Null
    });

    native!(reg, game, "SetSoundParamsFromLabel", |gm, ctx, a| {
        let id = h(a, 0);
        let label = str_at(ctx, a, 1);
        sound_set(&mut gm, id, "label", PropValue::Text(label.clone()), |s| {
            s.label = label
        });
        Value::Null
    });

    native!(reg, game, "SetSoundChannel", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), n(a, 1));
        sound_set(&mut gm, id, "channel", PropValue::Int(v), |s| s.channel = v);
        Value::Null
    });

    native!(reg, game, "SetSoundVolume", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), n(a, 1));
        sound_set(&mut gm, id, "volume", PropValue::Int(v), |s| s.volume = v);
        Value::Null
    });

    native!(reg, game, "SetSoundPitch", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), r(a, 1));
        sound_set(&mut gm, id, "pitch", PropValue::Real(v), |s| s.pitch = v);
        Value::Null
    });

    native!(reg, game, "SetSoundPlayPosition", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), n(a, 1));
        sound_set(&mut gm, id, "play_position_ms", PropValue::Int(v), |s| {
            s.play_position_ms = v
        });
        Value::Null
    });

    native!(reg, game, "SetSoundDistances", |gm, _ctx, a| {
        let (id, lo, hi) = (h(a, 0), r(a, 1), r(a, 2));
        sound_set(
            &mut gm,
            id,
            "distances",
            PropValue::Reals(vec![lo, hi]),
            |s| {
                s.min_distance = lo;
                s.max_distance = hi;
            },
        );
        Value::Null
    });

    native!(reg, game, "SetSoundDistanceCutoff", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), r(a, 1));
        sound_set(&mut gm, id, "distance_cutoff", PropValue::Real(v), |s| {
            s.distance_cutoff = v
        });
        Value::Null
    });

    native!(reg, game, "SetSoundConeAngles", |gm, _ctx, a| {
        let (id, inner, outer, vol) = (h(a, 0), r(a, 1), r(a, 2), n(a, 3));
        sound_set(
            &mut gm,
            id,
            "cone",
            PropValue::Reals(vec![inner, outer, vol as f32]),
            |s| {
                s.cone = (inner, outer, vol);
            },
        );
        Value::Null
    });

    native!(reg, game, "SetSoundConeOrientation", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), [r(a, 1), r(a, 2), r(a, 3)]);
        sound_set(
            &mut gm,
            id,
            "cone_orientation",
            PropValue::Reals(v.to_vec()),
            |s| s.cone_orientation = v,
        );
        Value::Null
    });

    native!(reg, game, "SetSoundPosition", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), [r(a, 1), r(a, 2), r(a, 3)]);
        sound_set(&mut gm, id, "position", PropValue::Reals(v.to_vec()), |s| {
            s.position = v
        });
        Value::Null
    });

    native!(reg, game, "SetSoundVelocity", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), [r(a, 1), r(a, 2), r(a, 3)]);
        sound_set(&mut gm, id, "velocity", PropValue::Reals(v.to_vec()), |s| {
            s.velocity = v
        });
        Value::Null
    });

    native!(reg, game, "AttachSoundToUnit", |gm, _ctx, a| {
        let (id, unit) = (h(a, 0), raw(a, 1));
        sound_set(&mut gm, id, "attached_unit", PropValue::Handle(unit), |s| {
            s.attached_unit = Some(unit)
        });
        Value::Null
    });

    native!(reg, game, "StartSound", |gm, _ctx, a| {
        let id = h(a, 0);
        if let Some(s) = sound_mut(&mut gm, id) {
            s.started = true;
            s.playing = true;
            log(&mut gm, PresentationEvent::SoundStarted { handle: id });
        }
        Value::Null
    });

    // A stopped sound is done at once here, because playback is not timed. With `killWhenDone` it
    // releases its handle right after the stop.
    native!(reg, game, "StopSound", |gm, _ctx, a| {
        let (id, fade_out, kill) = (h(a, 0), flag(a, 1), flag(a, 2));
        let mut stopped = false;
        if let Some(s) = sound_mut(&mut gm, id) {
            s.playing = false;
            s.kill_when_done |= kill;
            stopped = true;
        }
        if stopped {
            log(
                &mut gm,
                PresentationEvent::SoundStopped {
                    handle: id,
                    fade_out,
                    kill,
                },
            );
            if kill {
                gm.finish_sound(id);
            }
        }
        Value::Null
    });

    // A sound that is not playing has nothing to wait for, so its handle is released now (UNSURE:
    // the original's behaviour for a sound that never started). A playing sound is released by
    // `Game::finish_sound` when the host reports that it ended.
    native!(reg, game, "KillSoundWhenDone", |gm, _ctx, a| {
        let id = h(a, 0);
        let not_playing = match sound_mut(&mut gm, id) {
            Some(s) => {
                s.kill_when_done = true;
                !s.playing
            }
            None => false,
        };
        if not_playing {
            destroy(&mut gm, id, HandleKind::Sound, PresKind::Sound);
        }
        Value::Null
    });

    native!(reg, game, "SetSoundDuration", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), n(a, 1));
        sound_set(&mut gm, id, "duration_ms", PropValue::Int(v), |s| {
            s.duration_ms = v
        });
        Value::Null
    });

    native!(reg, game, "GetSoundDuration", |gm, _ctx, a| {
        Value::Integer(sound_mut(&mut gm, h(a, 0)).map_or(0, |s| s.duration_ms))
    });

    native!(reg, game, "GetSoundFileDuration", |gm, ctx, a| {
        let file = str_at(ctx, a, 0);
        Value::Integer(
            gm.presentation()
                .sound_file_durations
                .get(&file)
                .copied()
                .unwrap_or(0),
        )
    });

    native!(reg, game, "GetSoundIsPlaying", |gm, _ctx, a| {
        Value::Boolean(sound_mut(&mut gm, h(a, 0)).is_some_and(|s| s.playing))
    });

    // Sounds are not streamed here, so none is still loading (UNSURE).
    native!(reg, game, "GetSoundIsLoading", |_gm, _ctx, _a| {
        Value::Boolean(false)
    });

    // The group argument is not passed on in 1.26a (CONFIRMED): every group gets this volume.
    native!(reg, game, "VolumeGroupSetVolume", |gm, _ctx, a| {
        let v = r(a, 1);
        gm.presentation_mut().volume_group = v;
        command(&mut gm, "VolumeGroupVolume", vec![PropValue::Real(v)]);
        Value::Null
    });

    native!(reg, game, "VolumeGroupReset", |gm, _ctx, _a| {
        gm.presentation_mut().volume_group = 1.0;
        command(&mut gm, "VolumeGroupVolume", vec![PropValue::Real(1.0)]);
        Value::Null
    });

    native!(reg, game, "SetMapMusic", |gm, ctx, a| {
        let name = str_at(ctx, a, 0);
        let (random, index) = (flag(a, 1), n(a, 2));
        gm.presentation_mut().music.map_music = Some((name.clone(), random, index));
        log(
            &mut gm,
            PresentationEvent::MapMusicSet {
                name,
                random,
                index,
            },
        );
        Value::Null
    });

    native!(reg, game, "ClearMapMusic", |gm, _ctx, _a| {
        gm.presentation_mut().music.map_music = None;
        log(&mut gm, PresentationEvent::MapMusicCleared);
        Value::Null
    });

    native!(reg, game, "PlayMusic", |gm, ctx, a| {
        play_music(&mut gm, str_at(ctx, a, 0), false, 0, 0);
        Value::Null
    });

    native!(reg, game, "PlayMusicEx", |gm, ctx, a| {
        play_music(&mut gm, str_at(ctx, a, 0), false, n(a, 1), n(a, 2));
        Value::Null
    });

    native!(reg, game, "PlayThematicMusic", |gm, ctx, a| {
        play_music(&mut gm, str_at(ctx, a, 0), true, 0, 0);
        Value::Null
    });

    native!(reg, game, "PlayThematicMusicEx", |gm, ctx, a| {
        play_music(&mut gm, str_at(ctx, a, 0), true, n(a, 1), 0);
        Value::Null
    });

    native!(reg, game, "EndThematicMusic", |gm, _ctx, _a| {
        let m = &mut gm.presentation_mut().music;
        if m.thematic {
            m.current = None;
            m.thematic = false;
        }
        command(&mut gm, "EndThematicMusic", vec![]);
        Value::Null
    });

    native!(reg, game, "StopMusic", |gm, _ctx, a| {
        gm.presentation_mut().music.current = None;
        log(
            &mut gm,
            PresentationEvent::MusicStopped {
                fade_out: flag(a, 0),
            },
        );
        Value::Null
    });

    native!(reg, game, "ResumeMusic", |gm, _ctx, _a| {
        log(&mut gm, PresentationEvent::MusicResumed);
        Value::Null
    });

    native!(reg, game, "SetMusicVolume", |gm, _ctx, a| {
        let v = n(a, 0).clamp(0, 127);
        gm.presentation_mut().music.volume = v;
        command(&mut gm, "MusicVolume", vec![PropValue::Int(v)]);
        Value::Null
    });

    native!(reg, game, "SetMusicPlayPosition", |gm, _ctx, a| {
        gm.presentation_mut().music.play_position_ms = n(a, 0);
        command(&mut gm, "MusicPlayPosition", vec![PropValue::Int(n(a, 0))]);
        Value::Null
    });

    native!(reg, game, "SetThematicMusicPlayPosition", |gm, _ctx, a| {
        gm.presentation_mut().music.play_position_ms = n(a, 0);
        command(
            &mut gm,
            "ThematicMusicPlayPosition",
            vec![PropValue::Int(n(a, 0))],
        );
        Value::Null
    });

    native!(reg, game, "NewSoundEnvironment", |gm, ctx, a| {
        let name = str_at(ctx, a, 0);
        command(&mut gm, "NewSoundEnvironment", vec![PropValue::Text(name)]);
        Value::Null
    });
}

// ---------------------------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------------------------

fn register_text(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    // The message is shown to the player only if it is the local one (the renderer's check); all
    // messages are logged, with their player slot.
    native!(reg, game, "DisplayTextToPlayer", |gm, ctx, a| {
        if let Some(player) = player_slot(&gm, h(a, 0)) {
            let (x, y) = (r(a, 1), r(a, 2));
            let text = str_at(ctx, a, 3);
            log(
                &mut gm,
                PresentationEvent::Text {
                    player,
                    x,
                    y,
                    duration: None,
                    text,
                },
            );
        }
        Value::Null
    });

    native!(reg, game, "DisplayTimedTextToPlayer", |gm, ctx, a| {
        if let Some(player) = player_slot(&gm, h(a, 0)) {
            let (x, y, duration) = (r(a, 1), r(a, 2), r(a, 3));
            let text = str_at(ctx, a, 4);
            log(
                &mut gm,
                PresentationEvent::Text {
                    player,
                    x,
                    y,
                    duration: Some(duration),
                    text,
                },
            );
        }
        Value::Null
    });

    native!(reg, game, "DisplayTimedTextFromPlayer", |gm, ctx, a| {
        if let Some(player) = player_slot(&gm, h(a, 0)) {
            let (x, y, duration) = (r(a, 1), r(a, 2), r(a, 3));
            let text = str_at(ctx, a, 4);
            log(
                &mut gm,
                PresentationEvent::Text {
                    player,
                    x,
                    y,
                    duration: Some(duration),
                    text,
                },
            );
        }
        Value::Null
    });

    native!(reg, game, "ClearTextMessages", |gm, _ctx, _a| {
        log(&mut gm, PresentationEvent::TextMessagesCleared);
        Value::Null
    });

    native!(reg, game, "SetIntroShotText", |gm, ctx, a| {
        let text = str_at(ctx, a, 0);
        command(&mut gm, "IntroShotText", vec![PropValue::Text(text)]);
        Value::Null
    });

    native!(reg, game, "DisplayLoadDialog", |gm, _ctx, _a| {
        command(&mut gm, "DisplayLoadDialog", vec![]);
        Value::Null
    });

    native!(reg, game, "FlashQuestDialogButton", |gm, _ctx, _a| {
        command(&mut gm, "FlashQuestDialogButton", vec![]);
        Value::Null
    });

    native!(reg, game, "SetReservedLocalHeroButtons", |gm, _ctx, a| {
        let v = n(a, 0);
        gm.presentation_mut().ui.reserved_hero_buttons = v;
        command(&mut gm, "ReservedLocalHeroButtons", vec![PropValue::Int(v)]);
        Value::Null
    });
}

// ---------------------------------------------------------------------------------------------
// Handle objects: quests, multiboards, leaderboards, dialogs, timer dialogs, effects
// ---------------------------------------------------------------------------------------------

/// Quest flag bits (the original's `+0x24` word).
const QUEST_COMPLETED: u32 = 1;
const QUEST_REQUIRED: u32 = 2;
const QUEST_DISCOVERED: u32 = 4;
const QUEST_FAILED: u32 = 8;
const QUEST_ENABLED: u32 = 0x10;

/// Sets or clears one quest flag bit and logs it.
fn quest_flag_set(gm: &mut Game, id: u32, bit: u32, on: bool, prop: &'static str) {
    let Some(q) = quest_mut(gm, id) else {
        return;
    };
    if on {
        q.flags |= bit;
    } else {
        q.flags &= !bit;
    }
    set(gm, PresKind::Quest, id, prop, PropValue::Bool(on));
}

fn quest_flag_get(gm: &mut Game, id: u32, bit: u32) -> bool {
    quest_mut(gm, id).is_some_and(|q| q.flags & bit != 0)
}

/// Removes the dialog buttons of `keys`, releasing their handles.
fn kill_buttons(gm: &mut Game, keys: &[ObjKey]) {
    for &k in keys {
        let id = gm.handle_of(k);
        if gm.is_alive(k) {
            gm.kill(k);
            log(
                gm,
                PresentationEvent::Destroyed {
                    kind: PresKind::Button,
                    handle: id,
                },
            );
        }
    }
}

/// Sorts the leaderboard items with `key`, ascending or descending (UNSURE for the order of ties
/// and of the direction).
fn sort_leaderboard(gm: &mut Game, id: u32, ascending: bool, key: fn(&LeaderboardItem) -> SortKey) {
    if let Some(lb) = leaderboard_mut(gm, id) {
        lb.items.sort_by(|x, y| {
            let o = key(x).cmp(&key(y));
            if ascending {
                o
            } else {
                o.reverse()
            }
        });
    }
}

/// A sort key for leaderboard items: a label, a value or a player slot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    Text(String),
    Int(i64),
}

fn register_objects(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    // Quests
    native!(reg, game, "CreateQuest", |gm, _ctx, _a| {
        let obj = Quest {
            title: String::new(),
            description: String::new(),
            icon: String::new(),
            flags: QUEST_ENABLED,
        };
        let id = create(&mut gm, PresObj::Quest(obj), 0);
        log(
            &mut gm,
            PresentationEvent::Created {
                kind: PresKind::Quest,
                handle: id,
            },
        );
        Value::Handle(id)
    });

    native!(reg, game, "DestroyQuest", |gm, _ctx, a| {
        destroy(&mut gm, h(a, 0), HandleKind::Quest, PresKind::Quest);
        Value::Null
    });

    native!(reg, game, "QuestSetTitle", |gm, ctx, a| {
        let text = str_at(ctx, a, 1);
        if let Some(q) = quest_mut(&mut gm, h(a, 0)) {
            q.title = text.clone();
            set(
                &mut gm,
                PresKind::Quest,
                h(a, 0),
                "title",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    native!(reg, game, "QuestSetDescription", |gm, ctx, a| {
        let text = str_at(ctx, a, 1);
        if let Some(q) = quest_mut(&mut gm, h(a, 0)) {
            q.description = text.clone();
            set(
                &mut gm,
                PresKind::Quest,
                h(a, 0),
                "description",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    native!(reg, game, "QuestSetIconPath", |gm, ctx, a| {
        let text = str_at(ctx, a, 1);
        if let Some(q) = quest_mut(&mut gm, h(a, 0)) {
            q.icon = text.clone();
            set(
                &mut gm,
                PresKind::Quest,
                h(a, 0),
                "icon",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    native!(reg, game, "QuestSetRequired", |gm, _ctx, a| {
        quest_flag_set(&mut gm, h(a, 0), QUEST_REQUIRED, flag(a, 1), "required");
        Value::Null
    });
    native!(reg, game, "QuestSetCompleted", |gm, _ctx, a| {
        quest_flag_set(&mut gm, h(a, 0), QUEST_COMPLETED, flag(a, 1), "completed");
        Value::Null
    });
    native!(reg, game, "QuestSetDiscovered", |gm, _ctx, a| {
        quest_flag_set(&mut gm, h(a, 0), QUEST_DISCOVERED, flag(a, 1), "discovered");
        Value::Null
    });
    native!(reg, game, "QuestSetFailed", |gm, _ctx, a| {
        quest_flag_set(&mut gm, h(a, 0), QUEST_FAILED, flag(a, 1), "failed");
        Value::Null
    });
    native!(reg, game, "QuestSetEnabled", |gm, _ctx, a| {
        quest_flag_set(&mut gm, h(a, 0), QUEST_ENABLED, flag(a, 1), "enabled");
        Value::Null
    });

    native!(reg, game, "IsQuestRequired", |gm, _ctx, a| {
        Value::Boolean(quest_flag_get(&mut gm, h(a, 0), QUEST_REQUIRED))
    });
    native!(reg, game, "IsQuestCompleted", |gm, _ctx, a| {
        Value::Boolean(quest_flag_get(&mut gm, h(a, 0), QUEST_COMPLETED))
    });
    native!(reg, game, "IsQuestDiscovered", |gm, _ctx, a| {
        Value::Boolean(quest_flag_get(&mut gm, h(a, 0), QUEST_DISCOVERED))
    });
    native!(reg, game, "IsQuestFailed", |gm, _ctx, a| {
        Value::Boolean(quest_flag_get(&mut gm, h(a, 0), QUEST_FAILED))
    });
    native!(reg, game, "IsQuestEnabled", |gm, _ctx, a| {
        Value::Boolean(quest_flag_get(&mut gm, h(a, 0), QUEST_ENABLED))
    });

    native!(reg, game, "QuestCreateItem", |gm, _ctx, a| {
        let Some(quest) = gm.handle_of_kind(h(a, 0), HandleKind::Quest) else {
            return Value::Handle(0);
        };
        let obj = QuestItem {
            quest,
            description: String::new(),
            completed: false,
        };
        let id = create(&mut gm, PresObj::QuestItem(obj), 0);
        log(
            &mut gm,
            PresentationEvent::Created {
                kind: PresKind::QuestItem,
                handle: id,
            },
        );
        Value::Handle(id)
    });

    native!(reg, game, "QuestItemSetDescription", |gm, ctx, a| {
        let text = str_at(ctx, a, 1);
        if let Some(q) = quest_item_mut(&mut gm, h(a, 0)) {
            q.description = text.clone();
            set(
                &mut gm,
                PresKind::QuestItem,
                h(a, 0),
                "description",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    native!(reg, game, "QuestItemSetCompleted", |gm, _ctx, a| {
        let on = flag(a, 1);
        if let Some(q) = quest_item_mut(&mut gm, h(a, 0)) {
            q.completed = on;
            set(
                &mut gm,
                PresKind::QuestItem,
                h(a, 0),
                "completed",
                PropValue::Bool(on),
            );
        }
        Value::Null
    });

    native!(reg, game, "IsQuestItemCompleted", |gm, _ctx, a| {
        Value::Boolean(quest_item_mut(&mut gm, h(a, 0)).is_some_and(|q| q.completed))
    });

    // Multiboards. The row and column counts are the stored ones (+0x48 and +0x4c); the item
    // defaults apply to every item the board has no own setting for.
    native!(reg, game, "CreateMultiboard", |gm, _ctx, _a| {
        let id = create(&mut gm, PresObj::Multiboard(Multiboard::default()), 0);
        log(
            &mut gm,
            PresentationEvent::Created {
                kind: PresKind::Multiboard,
                handle: id,
            },
        );
        Value::Handle(id)
    });

    native!(reg, game, "DestroyMultiboard", |gm, _ctx, a| {
        destroy(
            &mut gm,
            h(a, 0),
            HandleKind::Multiboard,
            PresKind::Multiboard,
        );
        Value::Null
    });

    native!(reg, game, "MultiboardDisplay", |gm, _ctx, a| {
        let on = flag(a, 1);
        if let Some(m) = multiboard_mut(&mut gm, h(a, 0)) {
            m.displayed = on;
            set(
                &mut gm,
                PresKind::Multiboard,
                h(a, 0),
                "displayed",
                PropValue::Bool(on),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardMinimize", |gm, _ctx, a| {
        let on = flag(a, 1);
        if let Some(m) = multiboard_mut(&mut gm, h(a, 0)) {
            m.minimized = on;
            set(
                &mut gm,
                PresKind::Multiboard,
                h(a, 0),
                "minimized",
                PropValue::Bool(on),
            );
        }
        Value::Null
    });

    native!(reg, game, "IsMultiboardDisplayed", |gm, _ctx, a| {
        Value::Boolean(multiboard_mut(&mut gm, h(a, 0)).is_some_and(|m| m.displayed))
    });

    native!(reg, game, "IsMultiboardMinimized", |gm, _ctx, a| {
        Value::Boolean(multiboard_mut(&mut gm, h(a, 0)).is_some_and(|m| m.minimized))
    });

    // CONFIRMED: the original clears the board by setting both counts to 0.
    native!(reg, game, "MultiboardClear", |gm, _ctx, a| {
        if let Some(m) = multiboard_mut(&mut gm, h(a, 0)) {
            m.rows = 0;
            m.cols = 0;
            set(
                &mut gm,
                PresKind::Multiboard,
                h(a, 0),
                "rows",
                PropValue::Int(0),
            );
            set(
                &mut gm,
                PresKind::Multiboard,
                h(a, 0),
                "cols",
                PropValue::Int(0),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardSetTitleText", |gm, ctx, a| {
        let text = str_at(ctx, a, 1);
        if let Some(m) = multiboard_mut(&mut gm, h(a, 0)) {
            m.title = text.clone();
            set(
                &mut gm,
                PresKind::Multiboard,
                h(a, 0),
                "title",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardGetTitleText", |gm, ctx, a| {
        let title = multiboard_mut(&mut gm, h(a, 0))
            .map(|m| m.title.clone())
            .unwrap_or_default();
        vs(ctx, &title)
    });

    native!(reg, game, "MultiboardSetTitleTextColor", |gm, _ctx, a| {
        let c = argb(a, 1);
        if let Some(m) = multiboard_mut(&mut gm, h(a, 0)) {
            m.title_color = c;
            set(
                &mut gm,
                PresKind::Multiboard,
                h(a, 0),
                "title_color",
                PropValue::Color(c),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardGetRowCount", |gm, _ctx, a| {
        Value::Integer(multiboard_mut(&mut gm, h(a, 0)).map_or(0, |m| m.rows as i32))
    });

    native!(reg, game, "MultiboardGetColumnCount", |gm, _ctx, a| {
        Value::Integer(multiboard_mut(&mut gm, h(a, 0)).map_or(0, |m| m.cols as i32))
    });

    native!(reg, game, "MultiboardSetColumnCount", |gm, _ctx, a| {
        let v = n(a, 1).max(0) as u32;
        if let Some(m) = multiboard_mut(&mut gm, h(a, 0)) {
            m.cols = v;
            set(
                &mut gm,
                PresKind::Multiboard,
                h(a, 0),
                "cols",
                PropValue::Int(v as i32),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardSetRowCount", |gm, _ctx, a| {
        let v = n(a, 1).max(0) as u32;
        if let Some(m) = multiboard_mut(&mut gm, h(a, 0)) {
            m.rows = v;
            set(
                &mut gm,
                PresKind::Multiboard,
                h(a, 0),
                "rows",
                PropValue::Int(v as i32),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardSetItemsStyle", |gm, _ctx, a| {
        let (sv, si) = (flag(a, 1), flag(a, 2));
        if let Some(m) = multiboard_mut(&mut gm, h(a, 0)) {
            m.defaults.show_value = sv;
            m.defaults.show_icon = si;
            set(
                &mut gm,
                PresKind::Multiboard,
                h(a, 0),
                "items_style",
                PropValue::Reals(vec![sv as i32 as f32, si as i32 as f32]),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardSetItemsValueColor", |gm, _ctx, a| {
        let c = argb(a, 1);
        if let Some(m) = multiboard_mut(&mut gm, h(a, 0)) {
            m.defaults.value_color = c;
            set(
                &mut gm,
                PresKind::Multiboard,
                h(a, 0),
                "items_value_color",
                PropValue::Color(c),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardSetItemsValue", |gm, ctx, a| {
        let text = str_at(ctx, a, 1);
        if let Some(m) = multiboard_mut(&mut gm, h(a, 0)) {
            m.defaults.value = text.clone();
            set(
                &mut gm,
                PresKind::Multiboard,
                h(a, 0),
                "items_value",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardSetItemsWidth", |gm, _ctx, a| {
        let w = r(a, 1);
        if let Some(m) = multiboard_mut(&mut gm, h(a, 0)) {
            m.defaults.width = w;
            set(
                &mut gm,
                PresKind::Multiboard,
                h(a, 0),
                "items_width",
                PropValue::Real(w),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardSetItemsIcon", |gm, ctx, a| {
        let text = str_at(ctx, a, 1);
        if let Some(m) = multiboard_mut(&mut gm, h(a, 0)) {
            m.defaults.icon = text.clone();
            set(
                &mut gm,
                PresKind::Multiboard,
                h(a, 0),
                "items_icon",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    // The cell's item is created on the first request and the same handle is returned after that
    // (LIKELY: the original's allocator returns an object's existing id). Out-of-range cells are not
    // checked (UNSURE).
    native!(reg, game, "MultiboardGetItem", |gm, _ctx, a| {
        let Some(board) = gm.handle_of_kind(h(a, 0), HandleKind::Multiboard) else {
            return Value::Handle(0);
        };
        let (row, col) = (n(a, 1).max(0) as u32, n(a, 2).max(0) as u32);
        let existing = match gm.object(board) {
            Object::Pres(PresObj::Multiboard(m)) => m.cells.get(&(row, col)).copied(),
            _ => None,
        };
        if let Some(item) = existing.filter(|&k| gm.is_alive(k)) {
            return Value::Handle(gm.handle_of(item));
        }
        let obj = MultiboardItem {
            board,
            row,
            col,
            value: None,
            value_color: None,
            width: None,
            icon: None,
            style: None,
        };
        let (key, id) = gm.add_object(Object::Pres(PresObj::MultiboardItem(obj)), 0);
        if let Object::Pres(PresObj::Multiboard(m)) = gm.object_mut(board) {
            m.cells.insert((row, col), key);
        }
        log(
            &mut gm,
            PresentationEvent::Created {
                kind: PresKind::MultiboardItem,
                handle: id,
            },
        );
        Value::Handle(id)
    });

    native!(reg, game, "MultiboardReleaseItem", |gm, _ctx, a| {
        let id = h(a, 0);
        if let Some(k) = gm.handle_of_kind(id, HandleKind::MultiboardItem) {
            let (board, row, col) = match gm.object(k) {
                Object::Pres(PresObj::MultiboardItem(it)) => (it.board, it.row, it.col),
                _ => return Value::Null,
            };
            if let Object::Pres(PresObj::Multiboard(m)) = gm.object_mut(board) {
                if m.cells.get(&(row, col)) == Some(&k) {
                    m.cells.remove(&(row, col));
                }
            }
            destroy(
                &mut gm,
                id,
                HandleKind::MultiboardItem,
                PresKind::MultiboardItem,
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardSetItemStyle", |gm, _ctx, a| {
        let style = (flag(a, 1), flag(a, 2));
        if let Some(it) = multiboard_item_mut(&mut gm, h(a, 0)) {
            it.style = Some(style);
            set(
                &mut gm,
                PresKind::MultiboardItem,
                h(a, 0),
                "style",
                PropValue::Reals(vec![style.0 as i32 as f32, style.1 as i32 as f32]),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardSetItemValue", |gm, ctx, a| {
        let text = str_at(ctx, a, 1);
        if let Some(it) = multiboard_item_mut(&mut gm, h(a, 0)) {
            it.value = Some(text.clone());
            set(
                &mut gm,
                PresKind::MultiboardItem,
                h(a, 0),
                "value",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardSetItemValueColor", |gm, _ctx, a| {
        let c = argb(a, 1);
        if let Some(it) = multiboard_item_mut(&mut gm, h(a, 0)) {
            it.value_color = Some(c);
            set(
                &mut gm,
                PresKind::MultiboardItem,
                h(a, 0),
                "value_color",
                PropValue::Color(c),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardSetItemWidth", |gm, _ctx, a| {
        let w = r(a, 1);
        if let Some(it) = multiboard_item_mut(&mut gm, h(a, 0)) {
            it.width = Some(w);
            set(
                &mut gm,
                PresKind::MultiboardItem,
                h(a, 0),
                "width",
                PropValue::Real(w),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardSetItemIcon", |gm, ctx, a| {
        let text = str_at(ctx, a, 1);
        if let Some(it) = multiboard_item_mut(&mut gm, h(a, 0)) {
            it.icon = Some(text.clone());
            set(
                &mut gm,
                PresKind::MultiboardItem,
                h(a, 0),
                "icon",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    native!(reg, game, "MultiboardSuppressDisplay", |gm, _ctx, a| {
        let on = flag(a, 0);
        gm.presentation_mut().ui.multiboard_suppressed = on;
        command(
            &mut gm,
            "MultiboardSuppressDisplay",
            vec![PropValue::Bool(on)],
        );
        Value::Null
    });

    // Leaderboards. Items are kept in the order the sort natives give; a player has at most one item.
    native!(reg, game, "CreateLeaderboard", |gm, _ctx, _a| {
        let id = create(&mut gm, PresObj::Leaderboard(Leaderboard::default()), 0);
        log(
            &mut gm,
            PresentationEvent::Created {
                kind: PresKind::Leaderboard,
                handle: id,
            },
        );
        Value::Handle(id)
    });

    native!(reg, game, "DestroyLeaderboard", |gm, _ctx, a| {
        if let Some(k) = gm.handle_of_kind(h(a, 0), HandleKind::Leaderboard) {
            let links = &mut gm.presentation_mut().player_leaderboards;
            for link in links.iter_mut().filter(|l| **l == Some(k)) {
                *link = None;
            }
        }
        destroy(
            &mut gm,
            h(a, 0),
            HandleKind::Leaderboard,
            PresKind::Leaderboard,
        );
        Value::Null
    });

    native!(reg, game, "LeaderboardGetItemCount", |gm, _ctx, a| {
        Value::Integer(leaderboard_mut(&mut gm, h(a, 0)).map_or(0, |lb| lb.items.len() as i32))
    });

    native!(reg, game, "LeaderboardSetSizeByItemCount", |gm, _ctx, a| {
        let v = n(a, 1);
        if let Some(lb) = leaderboard_mut(&mut gm, h(a, 0)) {
            lb.size = v;
            set(
                &mut gm,
                PresKind::Leaderboard,
                h(a, 0),
                "size",
                PropValue::Int(v),
            );
        }
        Value::Null
    });

    // A player that already has an item gets its item replaced (UNSURE: the original may add a second).
    native!(reg, game, "LeaderboardAddItem", |gm, ctx, a| {
        let lb_id = h(a, 0);
        let label = str_at(ctx, a, 1);
        let value = n(a, 2);
        let slot = player_slot(&gm, h(a, 3));
        if let (Some(slot), Some(lb)) = (slot, leaderboard_mut(&mut gm, lb_id)) {
            let item = LeaderboardItem {
                player: Some(slot),
                label,
                value,
                label_color: 0xFFFF_FFFF,
                value_color: 0xFFFF_FFFF,
                show_label: true,
                show_value: true,
                show_icon: true,
            };
            let pos = lb.items.iter().position(|it| it.player == Some(slot));
            match pos {
                Some(i) => lb.items[i] = item,
                None => lb.items.push(item),
            }
            set(
                &mut gm,
                PresKind::Leaderboard,
                lb_id,
                "item_added",
                PropValue::Int(slot as i32),
            );
        }
        Value::Null
    });

    native!(reg, game, "LeaderboardRemoveItem", |gm, _ctx, a| {
        let (id, index) = (h(a, 0), n(a, 1));
        if let Some(lb) = leaderboard_mut(&mut gm, id) {
            if let Some(i) = usize::try_from(index).ok().filter(|&i| i < lb.items.len()) {
                lb.items.remove(i);
                set(
                    &mut gm,
                    PresKind::Leaderboard,
                    id,
                    "item_removed",
                    PropValue::Int(index),
                );
            }
        }
        Value::Null
    });

    native!(reg, game, "LeaderboardRemovePlayerItem", |gm, _ctx, a| {
        let id = h(a, 0);
        let Some(slot) = player_slot(&gm, h(a, 1)) else {
            return Value::Null;
        };
        if let Some(lb) = leaderboard_mut(&mut gm, id) {
            lb.items.retain(|it| it.player != Some(slot));
            set(
                &mut gm,
                PresKind::Leaderboard,
                id,
                "player_item_removed",
                PropValue::Int(slot as i32),
            );
        }
        Value::Null
    });

    native!(reg, game, "LeaderboardSortItemsByValue", |gm, _ctx, a| {
        sort_leaderboard(&mut gm, h(a, 0), flag(a, 1), |it| {
            SortKey::Int(it.value as i64)
        });
        Value::Null
    });

    native!(reg, game, "LeaderboardSortItemsByPlayer", |gm, _ctx, a| {
        sort_leaderboard(&mut gm, h(a, 0), flag(a, 1), |it| {
            SortKey::Int(it.player.map_or(-1, |p| p as i64))
        });
        Value::Null
    });

    native!(reg, game, "LeaderboardSortItemsByLabel", |gm, _ctx, a| {
        sort_leaderboard(&mut gm, h(a, 0), flag(a, 1), |it| {
            SortKey::Text(it.label.clone())
        });
        Value::Null
    });

    native!(reg, game, "LeaderboardClear", |gm, _ctx, a| {
        if let Some(lb) = leaderboard_mut(&mut gm, h(a, 0)) {
            lb.items.clear();
            set(
                &mut gm,
                PresKind::Leaderboard,
                h(a, 0),
                "items",
                PropValue::Int(0),
            );
        }
        Value::Null
    });

    native!(reg, game, "LeaderboardDisplay", |gm, _ctx, a| {
        let on = flag(a, 1);
        if let Some(lb) = leaderboard_mut(&mut gm, h(a, 0)) {
            lb.displayed = on;
            set(
                &mut gm,
                PresKind::Leaderboard,
                h(a, 0),
                "displayed",
                PropValue::Bool(on),
            );
        }
        Value::Null
    });

    native!(reg, game, "IsLeaderboardDisplayed", |gm, _ctx, a| {
        Value::Boolean(leaderboard_mut(&mut gm, h(a, 0)).is_some_and(|lb| lb.displayed))
    });

    native!(reg, game, "LeaderboardGetLabelText", |gm, ctx, a| {
        let label = leaderboard_mut(&mut gm, h(a, 0))
            .map(|lb| lb.label.clone())
            .unwrap_or_default();
        vs(ctx, &label)
    });

    native!(reg, game, "LeaderboardHasPlayerItem", |gm, _ctx, a| {
        let slot = player_slot(&gm, h(a, 1));
        let found = slot.is_some_and(|s| {
            leaderboard_mut(&mut gm, h(a, 0))
                .is_some_and(|lb| lb.items.iter().any(|it| it.player == Some(s)))
        });
        Value::Boolean(found)
    });

    native!(reg, game, "LeaderboardGetPlayerIndex", |gm, _ctx, a| {
        let slot = player_slot(&gm, h(a, 1));
        let index = match (slot, leaderboard_mut(&mut gm, h(a, 0))) {
            (Some(s), Some(lb)) => lb
                .items
                .iter()
                .position(|it| it.player == Some(s))
                .map_or(-1, |i| i as i32),
            _ => -1,
        };
        Value::Integer(index)
    });

    native!(reg, game, "LeaderboardSetLabel", |gm, ctx, a| {
        let text = str_at(ctx, a, 1);
        if let Some(lb) = leaderboard_mut(&mut gm, h(a, 0)) {
            lb.label = text.clone();
            set(
                &mut gm,
                PresKind::Leaderboard,
                h(a, 0),
                "label",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    native!(reg, game, "LeaderboardSetLabelColor", |gm, _ctx, a| {
        let c = argb(a, 1);
        if let Some(lb) = leaderboard_mut(&mut gm, h(a, 0)) {
            lb.label_color = c;
            set(
                &mut gm,
                PresKind::Leaderboard,
                h(a, 0),
                "label_color",
                PropValue::Color(c),
            );
        }
        Value::Null
    });

    native!(reg, game, "LeaderboardSetValueColor", |gm, _ctx, a| {
        let c = argb(a, 1);
        if let Some(lb) = leaderboard_mut(&mut gm, h(a, 0)) {
            lb.value_color = c;
            set(
                &mut gm,
                PresKind::Leaderboard,
                h(a, 0),
                "value_color",
                PropValue::Color(c),
            );
        }
        Value::Null
    });

    native!(reg, game, "LeaderboardSetStyle", |gm, _ctx, a| {
        let style = (flag(a, 1), flag(a, 2), flag(a, 3), flag(a, 4));
        if let Some(lb) = leaderboard_mut(&mut gm, h(a, 0)) {
            lb.style = style;
            let v = vec![bf(style.0), bf(style.1), bf(style.2), bf(style.3)];
            set(
                &mut gm,
                PresKind::Leaderboard,
                h(a, 0),
                "style",
                PropValue::Reals(v),
            );
        }
        Value::Null
    });

    native!(reg, game, "LeaderboardSetItemValue", |gm, _ctx, a| {
        let (id, index, v) = (h(a, 0), n(a, 1), n(a, 2));
        if let Some(it) = leaderboard_item_mut(&mut gm, id, index) {
            it.value = v;
            set(
                &mut gm,
                PresKind::Leaderboard,
                id,
                "item_value",
                PropValue::Int(v),
            );
        }
        Value::Null
    });

    native!(reg, game, "LeaderboardSetItemLabel", |gm, ctx, a| {
        let (id, index) = (h(a, 0), n(a, 1));
        let text = str_at(ctx, a, 2);
        if let Some(it) = leaderboard_item_mut(&mut gm, id, index) {
            it.label = text.clone();
            set(
                &mut gm,
                PresKind::Leaderboard,
                id,
                "item_label",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    native!(reg, game, "LeaderboardSetItemStyle", |gm, _ctx, a| {
        let (id, index) = (h(a, 0), n(a, 1));
        let (sl, sv, si) = (flag(a, 2), flag(a, 3), flag(a, 4));
        if let Some(it) = leaderboard_item_mut(&mut gm, id, index) {
            it.show_label = sl;
            it.show_value = sv;
            it.show_icon = si;
            set(
                &mut gm,
                PresKind::Leaderboard,
                id,
                "item_style",
                PropValue::Reals(vec![sl as i32 as f32, sv as i32 as f32, si as i32 as f32]),
            );
        }
        Value::Null
    });

    native!(reg, game, "LeaderboardSetItemLabelColor", |gm, _ctx, a| {
        let (id, index, c) = (h(a, 0), n(a, 1), argb(a, 2));
        if let Some(it) = leaderboard_item_mut(&mut gm, id, index) {
            it.label_color = c;
            set(
                &mut gm,
                PresKind::Leaderboard,
                id,
                "item_label_color",
                PropValue::Color(c),
            );
        }
        Value::Null
    });

    native!(reg, game, "LeaderboardSetItemValueColor", |gm, _ctx, a| {
        let (id, index, c) = (h(a, 0), n(a, 1), argb(a, 2));
        if let Some(it) = leaderboard_item_mut(&mut gm, id, index) {
            it.value_color = c;
            set(
                &mut gm,
                PresKind::Leaderboard,
                id,
                "item_value_color",
                PropValue::Color(c),
            );
        }
        Value::Null
    });

    // Dialogs. DialogDestroy and DialogClear release the buttons too (UNSURE for DialogDestroy).
    native!(reg, game, "DialogCreate", |gm, _ctx, _a| {
        let id = create(&mut gm, PresObj::Dialog(Dialog::default()), 0);
        log(
            &mut gm,
            PresentationEvent::Created {
                kind: PresKind::Dialog,
                handle: id,
            },
        );
        Value::Handle(id)
    });

    native!(reg, game, "DialogDestroy", |gm, _ctx, a| {
        let id = h(a, 0);
        if let Some(k) = gm.handle_of_kind(id, HandleKind::Dialog) {
            let buttons = match gm.object(k) {
                Object::Pres(PresObj::Dialog(d)) => d.buttons.clone(),
                _ => Vec::new(),
            };
            kill_buttons(&mut gm, &buttons);
        }
        destroy(&mut gm, id, HandleKind::Dialog, PresKind::Dialog);
        Value::Null
    });

    native!(reg, game, "DialogSetAsync", |gm, _ctx, a| {
        if let Some(d) = dialog_mut(&mut gm, h(a, 0)) {
            d.is_async = true;
            set(
                &mut gm,
                PresKind::Dialog,
                h(a, 0),
                "async",
                PropValue::Bool(true),
            );
        }
        Value::Null
    });

    native!(reg, game, "DialogClear", |gm, _ctx, a| {
        let id = h(a, 0);
        let buttons = match dialog_mut(&mut gm, id) {
            Some(d) => std::mem::take(&mut d.buttons),
            None => Vec::new(),
        };
        kill_buttons(&mut gm, &buttons);
        Value::Null
    });

    native!(reg, game, "DialogSetMessage", |gm, ctx, a| {
        let text = str_at(ctx, a, 1);
        if let Some(d) = dialog_mut(&mut gm, h(a, 0)) {
            d.message = text.clone();
            set(
                &mut gm,
                PresKind::Dialog,
                h(a, 0),
                "message",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    native!(reg, game, "DialogAddButton", |gm, ctx, a| {
        let dialog_id = h(a, 0);
        let Some(dialog) = gm.handle_of_kind(dialog_id, HandleKind::Dialog) else {
            return Value::Handle(0);
        };
        let obj = Button {
            dialog,
            text: str_at(ctx, a, 1),
            hotkey: n(a, 2),
            quit: false,
            score_screen: false,
        };
        let (key, id) = gm.add_object(Object::Pres(PresObj::Button(obj)), 0);
        if let Some(d) = dialog_mut(&mut gm, dialog_id) {
            d.buttons.push(key);
        }
        log(
            &mut gm,
            PresentationEvent::Created {
                kind: PresKind::Button,
                handle: id,
            },
        );
        Value::Handle(id)
    });

    native!(reg, game, "DialogAddQuitButton", |gm, ctx, a| {
        let dialog_id = h(a, 0);
        let Some(dialog) = gm.handle_of_kind(dialog_id, HandleKind::Dialog) else {
            return Value::Handle(0);
        };
        let obj = Button {
            dialog,
            text: str_at(ctx, a, 2),
            hotkey: n(a, 3),
            quit: true,
            score_screen: flag(a, 1),
        };
        let (key, id) = gm.add_object(Object::Pres(PresObj::Button(obj)), 0);
        if let Some(d) = dialog_mut(&mut gm, dialog_id) {
            d.buttons.push(key);
        }
        log(
            &mut gm,
            PresentationEvent::Created {
                kind: PresKind::Button,
                handle: id,
            },
        );
        Value::Handle(id)
    });

    native!(reg, game, "DialogDisplay", |gm, _ctx, a| {
        let (dialog_id, show) = (h(a, 1), flag(a, 2));
        let Some(slot) = player_slot(&gm, h(a, 0)) else {
            return Value::Null;
        };
        if let Some(d) = dialog_mut(&mut gm, dialog_id) {
            if show {
                d.shown_for |= 1 << slot;
            } else {
                d.shown_for &= !(1 << slot);
            }
            set(
                &mut gm,
                PresKind::Dialog,
                dialog_id,
                "shown_for_player",
                PropValue::Int(slot as i32),
            );
            set(
                &mut gm,
                PresKind::Dialog,
                dialog_id,
                "displayed",
                PropValue::Bool(show),
            );
        }
        Value::Null
    });

    // Timer dialogs show the timer of the handle they were made with (its remaining time is the
    // timer system's, not stored here).
    native!(reg, game, "CreateTimerDialog", |gm, _ctx, a| {
        let timer = gm.handle_of_kind(h(a, 0), HandleKind::Timer);
        let obj = TimerDialog {
            timer,
            ..TimerDialog::default()
        };
        let id = create(&mut gm, PresObj::TimerDialog(obj), 0);
        log(
            &mut gm,
            PresentationEvent::Created {
                kind: PresKind::TimerDialog,
                handle: id,
            },
        );
        Value::Handle(id)
    });

    native!(reg, game, "DestroyTimerDialog", |gm, _ctx, a| {
        destroy(
            &mut gm,
            h(a, 0),
            HandleKind::TimerDialog,
            PresKind::TimerDialog,
        );
        Value::Null
    });

    native!(reg, game, "TimerDialogDisplay", |gm, _ctx, a| {
        let on = flag(a, 1);
        if let Some(t) = timer_dialog_mut(&mut gm, h(a, 0)) {
            t.displayed = on;
            set(
                &mut gm,
                PresKind::TimerDialog,
                h(a, 0),
                "displayed",
                PropValue::Bool(on),
            );
        }
        Value::Null
    });

    native!(reg, game, "TimerDialogSetTitle", |gm, ctx, a| {
        let text = str_at(ctx, a, 1);
        if let Some(t) = timer_dialog_mut(&mut gm, h(a, 0)) {
            t.title = text.clone();
            set(
                &mut gm,
                PresKind::TimerDialog,
                h(a, 0),
                "title",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    native!(reg, game, "TimerDialogSetTitleColor", |gm, _ctx, a| {
        let c = argb(a, 1);
        if let Some(t) = timer_dialog_mut(&mut gm, h(a, 0)) {
            t.title_color = c;
            set(
                &mut gm,
                PresKind::TimerDialog,
                h(a, 0),
                "title_color",
                PropValue::Color(c),
            );
        }
        Value::Null
    });

    native!(reg, game, "TimerDialogSetTimeColor", |gm, _ctx, a| {
        let c = argb(a, 1);
        if let Some(t) = timer_dialog_mut(&mut gm, h(a, 0)) {
            t.time_color = c;
            set(
                &mut gm,
                PresKind::TimerDialog,
                h(a, 0),
                "time_color",
                PropValue::Color(c),
            );
        }
        Value::Null
    });

    native!(reg, game, "TimerDialogSetSpeed", |gm, _ctx, a| {
        let v = r(a, 1);
        if let Some(t) = timer_dialog_mut(&mut gm, h(a, 0)) {
            t.speed = v;
            set(
                &mut gm,
                PresKind::TimerDialog,
                h(a, 0),
                "speed",
                PropValue::Real(v),
            );
        }
        Value::Null
    });

    native!(
        reg,
        game,
        "TimerDialogSetRealTimeRemaining",
        |gm, _ctx, a| {
            let v = r(a, 1);
            if let Some(t) = timer_dialog_mut(&mut gm, h(a, 0)) {
                t.real_remaining = v;
                set(
                    &mut gm,
                    PresKind::TimerDialog,
                    h(a, 0),
                    "real_remaining",
                    PropValue::Real(v),
                );
            }
            Value::Null
        }
    );

    native!(reg, game, "IsTimerDialogDisplayed", |gm, _ctx, a| {
        Value::Boolean(timer_dialog_mut(&mut gm, h(a, 0)).is_some_and(|t| t.displayed))
    });

    // Special effects. The effect is a handle object (flag 0, from the original's `FUN_6f3bca30`).
    native!(reg, game, "AddSpecialEffect", |gm, ctx, a| {
        let model = str_at(ctx, a, 0);
        let at = EffectAt::Point {
            x: r(a, 1),
            y: r(a, 2),
        };
        Value::Handle(new_effect(&mut gm, model, at, None))
    });

    native!(reg, game, "AddSpecialEffectLoc", |gm, ctx, a| {
        let model = str_at(ctx, a, 0);
        match location_xy(&gm, h(a, 1)) {
            Some((x, y)) => {
                Value::Handle(new_effect(&mut gm, model, EffectAt::Point { x, y }, None))
            }
            None => null(),
        }
    });

    // The widget is any non-null handle (units are not in the simulation yet, so their ids are not
    // checked; UNSURE). The handle is allocated like the other effects (LIKELY).
    native!(reg, game, "AddSpecialEffectTarget", |gm, ctx, a| {
        let model = str_at(ctx, a, 0);
        let widget = raw(a, 1);
        if widget == 0 {
            return Value::Handle(0);
        }
        let attach = str_at(ctx, a, 2);
        let at = EffectAt::Target { widget, attach };
        Value::Handle(new_effect(&mut gm, model, at, None))
    });

    native!(reg, game, "DestroyEffect", |gm, _ctx, a| {
        destroy(&mut gm, h(a, 0), HandleKind::Effect, PresKind::Effect);
        Value::Null
    });

    // The spell forms name an ability (by string or by raw id) and an effect type. The original's
    // spell helper (`FUN_6f3bca30`) returns the same kind of handle as the special effects.
    native!(reg, game, "AddSpellEffect", |gm, ctx, a| {
        let model = str_at(ctx, a, 0);
        let at = EffectAt::Point {
            x: r(a, 2),
            y: r(a, 3),
        };
        Value::Handle(new_effect(&mut gm, model, at, Some(raw(a, 1))))
    });

    native!(reg, game, "AddSpellEffectLoc", |gm, ctx, a| {
        let model = str_at(ctx, a, 0);
        match location_xy(&gm, h(a, 2)) {
            Some((x, y)) => Value::Handle(new_effect(
                &mut gm,
                model,
                EffectAt::Point { x, y },
                Some(raw(a, 1)),
            )),
            None => null(),
        }
    });

    native!(reg, game, "AddSpellEffectById", |gm, _ctx, a| {
        let model = format!("#{}", n(a, 0));
        let at = EffectAt::Point {
            x: r(a, 2),
            y: r(a, 3),
        };
        Value::Handle(new_effect(&mut gm, model, at, Some(raw(a, 1))))
    });

    native!(reg, game, "AddSpellEffectByIdLoc", |gm, _ctx, a| {
        let model = format!("#{}", n(a, 0));
        match location_xy(&gm, h(a, 2)) {
            Some((x, y)) => Value::Handle(new_effect(
                &mut gm,
                model,
                EffectAt::Point { x, y },
                Some(raw(a, 1)),
            )),
            None => null(),
        }
    });

    // The original's target forms play the spell but return the null handle (CONFIRMED: the helper
    // they call, `FUN_6f3cd380`, returns 0). The play is logged with handle 0.
    native!(reg, game, "AddSpellEffectTarget", |gm, ctx, a| {
        let model = str_at(ctx, a, 0);
        let widget = raw(a, 2);
        let attach = str_at(ctx, a, 3);
        if widget != 0 {
            let at = EffectAt::Target { widget, attach };
            log(
                &mut gm,
                PresentationEvent::EffectSpawned {
                    handle: 0,
                    model,
                    at,
                },
            );
        }
        null()
    });

    native!(reg, game, "AddSpellEffectTargetById", |gm, ctx, a| {
        let model = format!("#{}", n(a, 0));
        let widget = raw(a, 2);
        let attach = str_at(ctx, a, 3);
        if widget != 0 {
            let at = EffectAt::Target { widget, attach };
            log(
                &mut gm,
                PresentationEvent::EffectSpawned {
                    handle: 0,
                    model,
                    at,
                },
            );
        }
        null()
    });
}

/// Creates an effect handle object and logs its spawn. Returns the handle id.
fn new_effect(gm: &mut Game, model: String, at: EffectAt, effect_type: Option<u32>) -> u32 {
    let obj = Effect {
        model: model.clone(),
        at: at.clone(),
        effect_type,
    };
    let id = create(gm, PresObj::Effect(obj), 0);
    log(
        gm,
        PresentationEvent::Created {
            kind: PresKind::Effect,
            handle: id,
        },
    );
    log(
        gm,
        PresentationEvent::EffectSpawned {
            handle: id,
            model,
            at,
        },
    );
    id
}

/// The leaderboard item `index`, when the handle is a live leaderboard and the index is in range.
fn leaderboard_item_mut(gm: &mut Game, id: u32, index: i32) -> Option<&mut LeaderboardItem> {
    let lb = leaderboard_mut(gm, id)?;
    let i = usize::try_from(index).ok()?;
    lb.items.get_mut(i)
}

/// A boolean as the 0 or 1 that the renderer reads from a `Reals` property.
fn bf(b: bool) -> f32 {
    if b {
        1.0
    } else {
        0.0
    }
}

// ---------------------------------------------------------------------------------------------
// Cinematic, interface, fog, sky and water
// ---------------------------------------------------------------------------------------------

fn register_visuals(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    native!(reg, game, "SetCineFilterTexture", |gm, ctx, a| {
        let texture = str_at(ctx, a, 0);
        gm.presentation_mut().cine.texture = texture.clone();
        command(&mut gm, "CineFilterTexture", vec![PropValue::Text(texture)]);
        Value::Null
    });

    // The original ignores values of 6 and above (CONFIRMED).
    native!(reg, game, "SetCineFilterBlendMode", |gm, _ctx, a| {
        let mode = raw(a, 0) as i32;
        if (0..6).contains(&mode) {
            gm.presentation_mut().cine.blend_mode = mode;
            command(&mut gm, "CineFilterBlendMode", vec![PropValue::Int(mode)]);
        }
        Value::Null
    });

    // The original ignores values of 4 and above (CONFIRMED).
    native!(reg, game, "SetCineFilterTexMapFlags", |gm, _ctx, a| {
        let flags = raw(a, 0) as i32;
        if (0..4).contains(&flags) {
            gm.presentation_mut().cine.tex_map_flags = flags;
            command(
                &mut gm,
                "CineFilterTexMapFlags",
                vec![PropValue::Int(flags)],
            );
        }
        Value::Null
    });

    native!(reg, game, "SetCineFilterStartUV", |gm, _ctx, a| {
        let uv = uv_sorted(a);
        gm.presentation_mut().cine.start_uv = uv;
        command(
            &mut gm,
            "CineFilterStartUV",
            vec![PropValue::Reals(uv.to_vec())],
        );
        Value::Null
    });

    native!(reg, game, "SetCineFilterEndUV", |gm, _ctx, a| {
        let uv = uv_sorted(a);
        gm.presentation_mut().cine.end_uv = uv;
        command(
            &mut gm,
            "CineFilterEndUV",
            vec![PropValue::Reals(uv.to_vec())],
        );
        Value::Null
    });

    native!(reg, game, "SetCineFilterStartColor", |gm, _ctx, a| {
        let c = argb(a, 0);
        gm.presentation_mut().cine.start_color = c;
        command(&mut gm, "CineFilterStartColor", vec![PropValue::Color(c)]);
        Value::Null
    });

    native!(reg, game, "SetCineFilterEndColor", |gm, _ctx, a| {
        let c = argb(a, 0);
        gm.presentation_mut().cine.end_color = c;
        command(&mut gm, "CineFilterEndColor", vec![PropValue::Color(c)]);
        Value::Null
    });

    native!(reg, game, "SetCineFilterDuration", |gm, _ctx, a| {
        let d = r(a, 0);
        gm.presentation_mut().cine.duration = d;
        command(&mut gm, "CineFilterDuration", vec![PropValue::Real(d)]);
        Value::Null
    });

    native!(reg, game, "DisplayCineFilter", |gm, _ctx, a| {
        let on = flag(a, 0);
        gm.presentation_mut().cine.displayed = on;
        command(&mut gm, "DisplayCineFilter", vec![PropValue::Bool(on)]);
        Value::Null
    });

    native!(reg, game, "IsCineFilterDisplayed", |gm, _ctx, _a| {
        Value::Boolean(gm.presentation().cine.displayed)
    });

    native!(reg, game, "SetCinematicScene", |gm, ctx, a| {
        let speaker = str_at(ctx, a, 2);
        let text = str_at(ctx, a, 3);
        let event = PresentationEvent::CinematicScene {
            portrait: n(a, 0),
            speaker,
            text,
            duration: r(a, 4),
            voiceover: r(a, 5),
        };
        log(&mut gm, event);
        Value::Null
    });

    native!(reg, game, "EndCinematicScene", |gm, _ctx, _a| {
        log(&mut gm, PresentationEvent::CinematicSceneEnded);
        Value::Null
    });

    native!(reg, game, "EnableUserControl", |gm, _ctx, a| {
        gm.presentation_mut().ui.user_control = flag(a, 0);
        command(&mut gm, "UserControl", vec![PropValue::Bool(flag(a, 0))]);
        Value::Null
    });

    native!(reg, game, "EnableUserUI", |gm, _ctx, a| {
        gm.presentation_mut().ui.user_ui = flag(a, 0);
        command(&mut gm, "UserUI", vec![PropValue::Bool(flag(a, 0))]);
        Value::Null
    });

    native!(reg, game, "ShowInterface", |gm, _ctx, a| {
        let (show, fade) = (flag(a, 0), r(a, 1));
        let ui = &mut gm.presentation_mut().ui;
        ui.interface_shown = show;
        ui.interface_fade = fade;
        command(
            &mut gm,
            "ShowInterface",
            vec![PropValue::Bool(show), PropValue::Real(fade)],
        );
        Value::Null
    });

    native!(reg, game, "ForceUICancel", |gm, _ctx, _a| {
        command(&mut gm, "ForceUICancel", vec![]);
        Value::Null
    });

    native!(reg, game, "ForceUIKey", |gm, ctx, a| {
        let key = str_at(ctx, a, 0);
        command(&mut gm, "ForceUIKey", vec![PropValue::Text(key)]);
        Value::Null
    });

    native!(reg, game, "ForceCinematicSubtitles", |gm, _ctx, a| {
        gm.presentation_mut().ui.cinematic_subtitles = flag(a, 0);
        command(
            &mut gm,
            "CinematicSubtitles",
            vec![PropValue::Bool(flag(a, 0))],
        );
        Value::Null
    });

    native!(reg, game, "PlayCinematic", |gm, ctx, a| {
        let name = str_at(ctx, a, 0);
        command(&mut gm, "PlayCinematic", vec![PropValue::Text(name)]);
        Value::Null
    });

    native!(reg, game, "PlayModelCinematic", |gm, ctx, a| {
        let name = str_at(ctx, a, 0);
        command(&mut gm, "PlayModelCinematic", vec![PropValue::Text(name)]);
        Value::Null
    });

    // CONFIRMED: `SetTerrainFog` has an empty body in 1.26a.
    native!(reg, game, "SetTerrainFog", |_gm, _ctx, _a| { Value::Null });

    native!(reg, game, "SetTerrainFogEx", |gm, _ctx, a| {
        // The colour is the original's packing of the three reals as bytes with full alpha.
        let c = |k: usize| ((r(a, k) * 255.0).round() as i32 & 0xff) as u32;
        let color = 0xFF00_0000 | (c(4) << 16) | (c(5) << 8) | c(6);
        let fog = TerrainFog {
            style: n(a, 0),
            z_start: r(a, 1),
            z_end: r(a, 2),
            density: r(a, 3),
            color,
        };
        let shown = PropValue::Reals(vec![fog.z_start, fog.z_end, fog.density]);
        gm.presentation_mut().fog.terrain_fog = Some(fog);
        command(
            &mut gm,
            "TerrainFog",
            vec![PropValue::Int(n(a, 0)), shown, PropValue::Color(color)],
        );
        Value::Null
    });

    native!(reg, game, "ResetTerrainFog", |gm, _ctx, _a| {
        gm.presentation_mut().fog.terrain_fog = None;
        command(&mut gm, "ResetTerrainFog", vec![]);
        Value::Null
    });

    native!(reg, game, "SetSkyModel", |gm, ctx, a| {
        let model = str_at(ctx, a, 0);
        gm.presentation_mut().fog.sky_model = model.clone();
        command(&mut gm, "SkyModel", vec![PropValue::Text(model)]);
        Value::Null
    });

    // The order of the two models follows the arguments (UNSURE: the original passes them in the
    // reverse order to its helper).
    native!(reg, game, "SetDayNightModels", |gm, ctx, a| {
        let pair = (str_at(ctx, a, 0), str_at(ctx, a, 1));
        gm.presentation_mut().fog.day_night_models = pair.clone();
        command(
            &mut gm,
            "DayNightModels",
            vec![PropValue::Text(pair.0), PropValue::Text(pair.1)],
        );
        Value::Null
    });

    native!(reg, game, "FogEnable", |gm, _ctx, a| {
        gm.presentation_mut().fog.fog_enabled = flag(a, 0);
        gm.fog.grid.set_fog_enabled(flag(a, 0));
        command(&mut gm, "FogEnable", vec![PropValue::Bool(flag(a, 0))]);
        Value::Null
    });

    native!(reg, game, "IsFogEnabled", |gm, _ctx, _a| {
        Value::Boolean(gm.presentation().fog.fog_enabled)
    });

    native!(reg, game, "FogMaskEnable", |gm, _ctx, a| {
        gm.presentation_mut().fog.mask_enabled = flag(a, 0);
        gm.fog.grid.set_mask_enabled(flag(a, 0));
        command(&mut gm, "FogMaskEnable", vec![PropValue::Bool(flag(a, 0))]);
        Value::Null
    });

    native!(reg, game, "IsFogMaskEnabled", |gm, _ctx, _a| {
        Value::Boolean(gm.presentation().fog.mask_enabled)
    });

    native!(reg, game, "EnableOcclusion", |gm, _ctx, a| {
        gm.presentation_mut().ui.occlusion = flag(a, 0);
        command(&mut gm, "Occlusion", vec![PropValue::Bool(flag(a, 0))]);
        Value::Null
    });

    native!(reg, game, "EnableWorldFogBoundary", |gm, _ctx, a| {
        gm.presentation_mut().ui.world_fog_boundary = flag(a, 0);
        command(
            &mut gm,
            "WorldFogBoundary",
            vec![PropValue::Bool(flag(a, 0))],
        );
        Value::Null
    });

    native!(reg, game, "SetWaterBaseColor", |gm, _ctx, a| {
        let c = argb(a, 0);
        gm.presentation_mut().fog.water_color = c;
        command(&mut gm, "WaterBaseColor", vec![PropValue::Color(c)]);
        Value::Null
    });

    native!(reg, game, "SetWaterDeforms", |gm, _ctx, a| {
        gm.presentation_mut().fog.water_deforms = flag(a, 0);
        command(&mut gm, "WaterDeforms", vec![PropValue::Bool(flag(a, 0))]);
        Value::Null
    });

    native!(reg, game, "SetAllyColorFilterState", |gm, _ctx, a| {
        gm.presentation_mut().ui.ally_color_filter = n(a, 0);
        command(
            &mut gm,
            "AllyColorFilterState",
            vec![PropValue::Int(n(a, 0))],
        );
        Value::Null
    });

    native!(reg, game, "GetAllyColorFilterState", |gm, _ctx, _a| {
        Value::Integer(gm.presentation().ui.ally_color_filter)
    });

    native!(reg, game, "GetCreepCampFilterState", |gm, _ctx, _a| {
        Value::Boolean(gm.presentation().ui.creep_camp_filter)
    });

    native!(reg, game, "SetCreepCampFilterState", |gm, _ctx, a| {
        gm.presentation_mut().ui.creep_camp_filter = flag(a, 0);
        command(
            &mut gm,
            "CreepCampFilterState",
            vec![PropValue::Bool(flag(a, 0))],
        );
        Value::Null
    });

    native!(reg, game, "EnableMinimapFilterButtons", |gm, _ctx, a| {
        let pair = (flag(a, 0), flag(a, 1));
        gm.presentation_mut().ui.minimap_filter = pair;
        command(
            &mut gm,
            "MinimapFilterButtons",
            vec![PropValue::Bool(pair.0), PropValue::Bool(pair.1)],
        );
        Value::Null
    });

    native!(reg, game, "PingMinimap", |gm, _ctx, a| {
        let args = vec![PropValue::Reals(vec![r(a, 0), r(a, 1), r(a, 2)])];
        command(&mut gm, "PingMinimap", args);
        Value::Null
    });

    native!(reg, game, "PingMinimapEx", |gm, _ctx, a| {
        let args = vec![
            PropValue::Reals(vec![r(a, 0), r(a, 1), r(a, 2)]),
            PropValue::Color(argb(a, 3)),
            PropValue::Bool(flag(a, 6)),
        ];
        command(&mut gm, "PingMinimapEx", args);
        Value::Null
    });
}

// ---------------------------------------------------------------------------------------------
// Pool objects: text tags, lightning, images, ubersplats, weather, terrain deformation
// ---------------------------------------------------------------------------------------------

fn register_pools(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    // Text tags
    native!(reg, game, "CreateTextTag", |gm, _ctx, _a| {
        let id = gm.presentation_mut().text_tags.insert(TextTag::default());
        log(
            &mut gm,
            PresentationEvent::Created {
                kind: PresKind::TextTag,
                handle: id,
            },
        );
        Value::Handle(id)
    });

    native!(reg, game, "DestroyTextTag", |gm, _ctx, a| {
        let id = h(a, 0);
        if gm.presentation_mut().text_tags.remove(id) {
            log(
                &mut gm,
                PresentationEvent::Destroyed {
                    kind: PresKind::TextTag,
                    handle: id,
                },
            );
        }
        Value::Null
    });

    native!(reg, game, "SetTextTagText", |gm, ctx, a| {
        let (id, text, size) = (h(a, 0), str_at(ctx, a, 1), r(a, 2));
        if let Some(t) = gm.presentation_mut().text_tags.get_mut(id) {
            t.text = text.clone();
            t.size = size;
            set(
                &mut gm,
                PresKind::TextTag,
                id,
                "text",
                PropValue::Text(text),
            );
        }
        Value::Null
    });

    native!(reg, game, "SetTextTagPos", |gm, _ctx, a| {
        let (id, pos) = (h(a, 0), [r(a, 1), r(a, 2), r(a, 3)]);
        if let Some(t) = gm.presentation_mut().text_tags.get_mut(id) {
            t.pos = pos;
            t.unit = None;
            set(
                &mut gm,
                PresKind::TextTag,
                id,
                "pos",
                PropValue::Reals(pos.to_vec()),
            );
        }
        Value::Null
    });

    native!(reg, game, "SetTextTagPosUnit", |gm, _ctx, a| {
        let (id, unit, height) = (h(a, 0), raw(a, 1), r(a, 2));
        if let Some(t) = gm.presentation_mut().text_tags.get_mut(id) {
            t.unit = Some(unit);
            t.pos[2] = height;
            set(
                &mut gm,
                PresKind::TextTag,
                id,
                "attached_unit",
                PropValue::Handle(unit),
            );
        }
        Value::Null
    });

    native!(reg, game, "SetTextTagColor", |gm, _ctx, a| {
        let (id, c) = (h(a, 0), argb(a, 1));
        if let Some(t) = gm.presentation_mut().text_tags.get_mut(id) {
            t.color = c;
            set(&mut gm, PresKind::TextTag, id, "color", PropValue::Color(c));
        }
        Value::Null
    });

    native!(reg, game, "SetTextTagVelocity", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), [r(a, 1), r(a, 2)]);
        if let Some(t) = gm.presentation_mut().text_tags.get_mut(id) {
            t.velocity = v;
            set(
                &mut gm,
                PresKind::TextTag,
                id,
                "velocity",
                PropValue::Reals(v.to_vec()),
            );
        }
        Value::Null
    });

    native!(reg, game, "SetTextTagVisibility", |gm, _ctx, a| {
        let (id, on) = (h(a, 0), flag(a, 1));
        if let Some(t) = gm.presentation_mut().text_tags.get_mut(id) {
            t.visible = on;
            set(
                &mut gm,
                PresKind::TextTag,
                id,
                "visible",
                PropValue::Bool(on),
            );
        }
        Value::Null
    });

    native!(reg, game, "SetTextTagSuspended", |gm, _ctx, a| {
        let (id, on) = (h(a, 0), flag(a, 1));
        if let Some(t) = gm.presentation_mut().text_tags.get_mut(id) {
            t.suspended = on;
            set(
                &mut gm,
                PresKind::TextTag,
                id,
                "suspended",
                PropValue::Bool(on),
            );
        }
        Value::Null
    });

    native!(reg, game, "SetTextTagPermanent", |gm, _ctx, a| {
        let (id, on) = (h(a, 0), flag(a, 1));
        if let Some(t) = gm.presentation_mut().text_tags.get_mut(id) {
            t.permanent = on;
            set(
                &mut gm,
                PresKind::TextTag,
                id,
                "permanent",
                PropValue::Bool(on),
            );
        }
        Value::Null
    });

    native!(reg, game, "SetTextTagAge", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), r(a, 1));
        if let Some(t) = gm.presentation_mut().text_tags.get_mut(id) {
            t.age = v;
            set(&mut gm, PresKind::TextTag, id, "age", PropValue::Real(v));
        }
        Value::Null
    });

    native!(reg, game, "SetTextTagLifespan", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), r(a, 1));
        if let Some(t) = gm.presentation_mut().text_tags.get_mut(id) {
            t.lifespan = v;
            set(
                &mut gm,
                PresKind::TextTag,
                id,
                "lifespan",
                PropValue::Real(v),
            );
        }
        Value::Null
    });

    native!(reg, game, "SetTextTagFadepoint", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), r(a, 1));
        if let Some(t) = gm.presentation_mut().text_tags.get_mut(id) {
            t.fadepoint = v;
            set(
                &mut gm,
                PresKind::TextTag,
                id,
                "fadepoint",
                PropValue::Real(v),
            );
        }
        Value::Null
    });

    // Lightning. A failed creation (an empty type name) gives 0.
    native!(reg, game, "AddLightning", |gm, ctx, a| {
        let kind = str_at(ctx, a, 0);
        let points = [r(a, 2), r(a, 3), 0.0, r(a, 4), r(a, 5), 0.0];
        Value::Handle(new_lightning(&mut gm, kind, flag(a, 1), points))
    });

    native!(reg, game, "AddLightningEx", |gm, ctx, a| {
        let kind = str_at(ctx, a, 0);
        let points = [r(a, 2), r(a, 3), r(a, 4), r(a, 5), r(a, 6), r(a, 7)];
        Value::Handle(new_lightning(&mut gm, kind, flag(a, 1), points))
    });

    native!(reg, game, "DestroyLightning", |gm, _ctx, a| {
        let id = h(a, 0);
        let freed = gm.presentation_mut().lightnings.remove(id);
        if freed {
            log(
                &mut gm,
                PresentationEvent::Destroyed {
                    kind: PresKind::Lightning,
                    handle: id,
                },
            );
        }
        Value::Boolean(freed)
    });

    native!(reg, game, "GetLightningColorR", |gm, _ctx, a| {
        vr(lightning_channel(&gm, h(a, 0), 0))
    });
    native!(reg, game, "GetLightningColorG", |gm, _ctx, a| {
        vr(lightning_channel(&gm, h(a, 0), 1))
    });
    native!(reg, game, "GetLightningColorB", |gm, _ctx, a| {
        vr(lightning_channel(&gm, h(a, 0), 2))
    });
    native!(reg, game, "GetLightningColorA", |gm, _ctx, a| {
        vr(lightning_channel(&gm, h(a, 0), 3))
    });

    native!(reg, game, "MoveLightning", |gm, _ctx, a| {
        let id = h(a, 0);
        let points = [r(a, 2), r(a, 3), 0.0, r(a, 4), r(a, 5), 0.0];
        Value::Boolean(move_lightning(&mut gm, id, flag(a, 1), points))
    });

    native!(reg, game, "MoveLightningEx", |gm, _ctx, a| {
        let id = h(a, 0);
        let points = [r(a, 2), r(a, 3), r(a, 4), r(a, 5), r(a, 6), r(a, 7)];
        Value::Boolean(move_lightning(&mut gm, id, flag(a, 1), points))
    });

    native!(reg, game, "SetLightningColor", |gm, _ctx, a| {
        let id = h(a, 0);
        let color = rgba_bytes(a, 1);
        let found = match gm.presentation_mut().lightnings.get_mut(id) {
            Some(l) => {
                l.color = color;
                true
            }
            None => false,
        };
        if found {
            set(
                &mut gm,
                PresKind::Lightning,
                id,
                "color",
                PropValue::Color(argb_of(color)),
            );
        }
        Value::Boolean(found)
    });

    // Images. The position, size and origin are stored as given.
    native!(reg, game, "CreateImage", |gm, ctx, a| {
        let image = Image {
            file: str_at(ctx, a, 0),
            size: [r(a, 1), r(a, 2), r(a, 3)],
            position: [r(a, 4), r(a, 5), r(a, 6)],
            origin: [r(a, 7), r(a, 8), r(a, 9)],
            image_type: n(a, 10),
            color: 0xFFFF_FFFF,
            shown: true,
            render: false,
            render_always: false,
            above_water: false,
            constant_height: None,
        };
        let id = gm.presentation_mut().images.insert(image);
        log(
            &mut gm,
            PresentationEvent::Created {
                kind: PresKind::Image,
                handle: id,
            },
        );
        Value::Handle(id)
    });

    native!(reg, game, "DestroyImage", |gm, _ctx, a| {
        destroy_pool(&mut gm, h(a, 0), PresKind::Image, |p| {
            p.images.remove(h_id(a))
        });
        Value::Null
    });

    native!(reg, game, "ShowImage", |gm, _ctx, a| {
        let (id, on) = (h(a, 0), flag(a, 1));
        pool_set(
            &mut gm,
            id,
            PresKind::Image,
            "shown",
            PropValue::Bool(on),
            |p| p.images.get_mut(id).map(|i| i.shown = on),
        );
        Value::Null
    });

    native!(reg, game, "SetImageConstantHeight", |gm, _ctx, a| {
        let (id, on, height) = (h(a, 0), flag(a, 1), r(a, 2));
        let v = if on { Some(height) } else { None };
        pool_set(
            &mut gm,
            id,
            PresKind::Image,
            "constant_height",
            PropValue::Reals(vec![bf(on), height]),
            |p| p.images.get_mut(id).map(|i| i.constant_height = v),
        );
        Value::Null
    });

    native!(reg, game, "SetImagePosition", |gm, _ctx, a| {
        let (id, pos) = (h(a, 0), [r(a, 1), r(a, 2), r(a, 3)]);
        pool_set(
            &mut gm,
            id,
            PresKind::Image,
            "position",
            PropValue::Reals(pos.to_vec()),
            |p| p.images.get_mut(id).map(|i| i.position = pos),
        );
        Value::Null
    });

    native!(reg, game, "SetImageColor", |gm, _ctx, a| {
        let (id, c) = (h(a, 0), argb(a, 1));
        pool_set(
            &mut gm,
            id,
            PresKind::Image,
            "color",
            PropValue::Color(c),
            |p| p.images.get_mut(id).map(|i| i.color = c),
        );
        Value::Null
    });

    native!(reg, game, "SetImageRender", |gm, _ctx, a| {
        let (id, on) = (h(a, 0), flag(a, 1));
        pool_set(
            &mut gm,
            id,
            PresKind::Image,
            "render",
            PropValue::Bool(on),
            |p| p.images.get_mut(id).map(|i| i.render = on),
        );
        Value::Null
    });

    native!(reg, game, "SetImageRenderAlways", |gm, _ctx, a| {
        let (id, on) = (h(a, 0), flag(a, 1));
        pool_set(
            &mut gm,
            id,
            PresKind::Image,
            "render_always",
            PropValue::Bool(on),
            |p| p.images.get_mut(id).map(|i| i.render_always = on),
        );
        Value::Null
    });

    native!(reg, game, "SetImageAboveWater", |gm, _ctx, a| {
        let (id, on) = (h(a, 0), flag(a, 1));
        pool_set(
            &mut gm,
            id,
            PresKind::Image,
            "above_water",
            PropValue::Bool(on),
            |p| p.images.get_mut(id).map(|i| i.above_water = on),
        );
        Value::Null
    });

    native!(reg, game, "SetImageType", |gm, _ctx, a| {
        let (id, v) = (h(a, 0), n(a, 1));
        pool_set(
            &mut gm,
            id,
            PresKind::Image,
            "type",
            PropValue::Int(v),
            |p| p.images.get_mut(id).map(|i| i.image_type = v),
        );
        Value::Null
    });

    // Ubersplats. Colour arguments are r, g, b, a (integers); the two flags follow.
    native!(reg, game, "CreateUbersplat", |gm, ctx, a| {
        let splat = Ubersplat {
            name: str_at(ctx, a, 2),
            position: [r(a, 0), r(a, 1)],
            color: argb(a, 3),
            shown: true,
            render: flag(a, 7),
            render_always: flag(a, 8),
            finished: false,
        };
        let id = gm.presentation_mut().ubersplats.insert(splat);
        log(
            &mut gm,
            PresentationEvent::Created {
                kind: PresKind::Ubersplat,
                handle: id,
            },
        );
        Value::Handle(id)
    });

    native!(reg, game, "DestroyUbersplat", |gm, _ctx, a| {
        destroy_pool(&mut gm, h(a, 0), PresKind::Ubersplat, |p| {
            p.ubersplats.remove(h_id(a))
        });
        Value::Null
    });

    native!(reg, game, "ResetUbersplat", |gm, _ctx, a| {
        let id = h(a, 0);
        pool_set(
            &mut gm,
            id,
            PresKind::Ubersplat,
            "reset",
            PropValue::Bool(true),
            |p| p.ubersplats.get_mut(id).map(|u| u.finished = false),
        );
        Value::Null
    });

    native!(reg, game, "FinishUbersplat", |gm, _ctx, a| {
        let id = h(a, 0);
        pool_set(
            &mut gm,
            id,
            PresKind::Ubersplat,
            "finished",
            PropValue::Bool(true),
            |p| p.ubersplats.get_mut(id).map(|u| u.finished = true),
        );
        Value::Null
    });

    native!(reg, game, "ShowUbersplat", |gm, _ctx, a| {
        let (id, on) = (h(a, 0), flag(a, 1));
        pool_set(
            &mut gm,
            id,
            PresKind::Ubersplat,
            "shown",
            PropValue::Bool(on),
            |p| p.ubersplats.get_mut(id).map(|u| u.shown = on),
        );
        Value::Null
    });

    native!(reg, game, "SetUbersplatRender", |gm, _ctx, a| {
        let (id, on) = (h(a, 0), flag(a, 1));
        pool_set(
            &mut gm,
            id,
            PresKind::Ubersplat,
            "render",
            PropValue::Bool(on),
            |p| p.ubersplats.get_mut(id).map(|u| u.render = on),
        );
        Value::Null
    });

    native!(reg, game, "SetUbersplatRenderAlways", |gm, _ctx, a| {
        let (id, on) = (h(a, 0), flag(a, 1));
        pool_set(
            &mut gm,
            id,
            PresKind::Ubersplat,
            "render_always",
            PropValue::Bool(on),
            |p| p.ubersplats.get_mut(id).map(|u| u.render_always = on),
        );
        Value::Null
    });

    // Weather over a rectangle. The rectangle is copied; an invalid rectangle gives 0.
    native!(reg, game, "AddWeatherEffect", |gm, _ctx, a| {
        match rect_edges(&gm, h(a, 0)) {
            Some(rect) => {
                let id = gm.presentation_mut().weather.insert(Weather {
                    rect,
                    effect_id: n(a, 1),
                    enabled: true,
                });
                log(
                    &mut gm,
                    PresentationEvent::Created {
                        kind: PresKind::WeatherEffect,
                        handle: id,
                    },
                );
                Value::Handle(id)
            }
            None => null(),
        }
    });

    native!(reg, game, "EnableWeatherEffect", |gm, _ctx, a| {
        let (id, on) = (h(a, 0), flag(a, 1));
        pool_set(
            &mut gm,
            id,
            PresKind::WeatherEffect,
            "enabled",
            PropValue::Bool(on),
            |p| p.weather.get_mut(id).map(|w| w.enabled = on),
        );
        Value::Null
    });

    native!(reg, game, "RemoveWeatherEffect", |gm, _ctx, a| {
        destroy_pool(&mut gm, h(a, 0), PresKind::WeatherEffect, |p| {
            p.weather.remove(h_id(a))
        });
        Value::Null
    });

    native!(reg, game, "TerrainDeformCrater", |gm, _ctx, a| {
        let params = vec![r(a, 0), r(a, 1), r(a, 2), r(a, 3), bf(flag(a, 5))];
        let id = new_deformation(&mut gm, "crater", params, n(a, 4) as f32);
        Value::Handle(id)
    });

    native!(reg, game, "TerrainDeformRipple", |gm, _ctx, a| {
        let params = vec![
            r(a, 0),
            r(a, 1),
            r(a, 2),
            r(a, 3),
            r(a, 6),
            r(a, 7),
            r(a, 8),
            n(a, 5) as f32,
            bf(flag(a, 9)),
        ];
        let id = new_deformation(&mut gm, "ripple", params, n(a, 4) as f32);
        Value::Handle(id)
    });

    native!(reg, game, "TerrainDeformWave", |gm, _ctx, a| {
        let mut params: Vec<f32> = (0..8).map(|i| r(a, i)).collect();
        params.push(n(a, 9) as f32);
        let id = new_deformation(&mut gm, "wave", params, n(a, 8) as f32);
        Value::Handle(id)
    });

    native!(reg, game, "TerrainDeformRandom", |gm, _ctx, a| {
        let mut params: Vec<f32> = (0..5).map(|i| r(a, i)).collect();
        params.push(n(a, 6) as f32);
        let id = new_deformation(&mut gm, "random", params, n(a, 5) as f32);
        Value::Handle(id)
    });

    native!(reg, game, "TerrainDeformStop", |gm, _ctx, a| {
        let (id, duration) = (h(a, 0), n(a, 1) as f32);
        pool_set(
            &mut gm,
            id,
            PresKind::TerrainDeformation,
            "stopped",
            PropValue::Real(duration),
            |p| {
                // The duration argument is the fade time of the stop; it is logged with the property.
                p.deformations.get_mut(id).map(|d| d.stopped = true)
            },
        );
        Value::Null
    });

    native!(reg, game, "TerrainDeformStopAll", |gm, _ctx, _a| {
        for d in gm.presentation_mut().deformations.iter_mut() {
            d.stopped = true;
        }
        command(&mut gm, "TerrainDeformStopAll", vec![]);
        Value::Null
    });
}

// ---------------------------------------------------------------------------------------------
// Other presentation settings
// ---------------------------------------------------------------------------------------------

fn register_misc(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    native!(reg, game, "SetAltMinimapIcon", |gm, ctx, a| {
        let path = str_at(ctx, a, 0);
        command(&mut gm, "AltMinimapIcon", vec![PropValue::Text(path)]);
        Value::Null
    });

    native!(reg, game, "SetIntroShotModel", |gm, ctx, a| {
        let model = str_at(ctx, a, 0);
        command(&mut gm, "IntroShotModel", vec![PropValue::Text(model)]);
        Value::Null
    });

    native!(reg, game, "DoNotSaveReplay", |gm, _ctx, _a| {
        command(&mut gm, "DoNotSaveReplay", vec![]);
        Value::Null
    });

    native!(reg, game, "ForceQuestDialogUpdate", |gm, _ctx, _a| {
        command(&mut gm, "ForceQuestDialogUpdate", vec![]);
        Value::Null
    });

    native!(reg, game, "EnableSelect", |gm, _ctx, a| {
        command(
            &mut gm,
            "EnableSelect",
            vec![PropValue::Bool(flag(a, 0)), PropValue::Bool(flag(a, 1))],
        );
        Value::Null
    });

    native!(reg, game, "EnablePreSelect", |gm, _ctx, a| {
        command(
            &mut gm,
            "EnablePreSelect",
            vec![PropValue::Bool(flag(a, 0)), PropValue::Bool(flag(a, 1))],
        );
        Value::Null
    });

    native!(reg, game, "EnableDragSelect", |gm, _ctx, a| {
        command(
            &mut gm,
            "EnableDragSelect",
            vec![PropValue::Bool(flag(a, 0)), PropValue::Bool(flag(a, 1))],
        );
        Value::Null
    });

    native!(reg, game, "SetTimeOfDayScale", |gm, _ctx, a| {
        let v = r(a, 0);
        gm.presentation_mut().time_of_day_scale = v;
        command(&mut gm, "TimeOfDayScale", vec![PropValue::Real(v)]);
        Value::Null
    });

    native!(reg, game, "GetTimeOfDayScale", |gm, _ctx, _a| {
        vr(gm.presentation().time_of_day_scale)
    });

    native!(
        reg,
        game,
        "SetCustomCampaignButtonVisible",
        |gm, _ctx, a| {
            let (index, on) = (n(a, 0), flag(a, 1));
            gm.presentation_mut()
                .custom_campaign_buttons
                .insert(index, on);
            command(
                &mut gm,
                "CustomCampaignButtonVisible",
                vec![PropValue::Int(index), PropValue::Bool(on)],
            );
            Value::Null
        }
    );

    native!(
        reg,
        game,
        "GetCustomCampaignButtonVisible",
        |gm, _ctx, a| {
            let index = n(a, 0);
            Value::Boolean(
                gm.presentation()
                    .custom_campaign_buttons
                    .get(&index)
                    .copied()
                    .unwrap_or(false),
            )
        }
    );

    // The player's leaderboard is stored as the leaderboard object. Getting it gives the object's
    // existing handle (the original's allocator returns the id an object already has, CONFIRMED
    // from `FUN_6f430c80`), or 0 when the leaderboard was destroyed.
    native!(reg, game, "PlayerSetLeaderboard", |gm, _ctx, a| {
        let Some(slot) = player_slot(&gm, h(a, 0)) else {
            return Value::Null;
        };
        let lb = gm.handle_of_kind(h(a, 1), HandleKind::Leaderboard);
        if let Some(lb) = lb {
            gm.presentation_mut().player_leaderboards[slot as usize] = Some(lb);
            set(
                &mut gm,
                PresKind::Leaderboard,
                h(a, 1),
                "player",
                PropValue::Int(slot as i32),
            );
        }
        Value::Null
    });

    native!(reg, game, "PlayerGetLeaderboard", |gm, _ctx, a| {
        let Some(slot) = player_slot(&gm, h(a, 0)) else {
            return null();
        };
        match gm.presentation().player_leaderboards[slot as usize] {
            Some(k) if gm.is_alive(k) => Value::Handle(gm.handle_of(k)),
            _ => null(),
        }
    });
}

/// The handle value of argument 0 (the pool closures need it without the `a` borrow).
fn h_id(a: &[Value]) -> u32 {
    h(a, 0)
}

/// Creates a terrain deformation and returns its pool handle.
fn new_deformation(gm: &mut Game, kind: &'static str, params: Vec<f32>, duration: f32) -> u32 {
    let id = gm
        .presentation_mut()
        .deformations
        .insert(TerrainDeformation {
            kind,
            params,
            duration,
            stopped: false,
        });
    log(
        gm,
        PresentationEvent::Created {
            kind: PresKind::TerrainDeformation,
            handle: id,
        },
    );
    id
}

/// Creates a lightning bolt. An empty type name gives the null handle (0).
fn new_lightning(gm: &mut Game, kind: String, check_visibility: bool, points: [f32; 6]) -> u32 {
    if kind.is_empty() {
        return 0;
    }
    let id = gm.presentation_mut().lightnings.insert(Lightning {
        kind,
        check_visibility,
        points,
        color: [255; 4],
    });
    log(
        gm,
        PresentationEvent::Created {
            kind: PresKind::Lightning,
            handle: id,
        },
    );
    id
}

/// Moves a bolt. Returns whether the bolt exists.
fn move_lightning(gm: &mut Game, id: u32, check_visibility: bool, points: [f32; 6]) -> bool {
    let found = match gm.presentation_mut().lightnings.get_mut(id) {
        Some(l) => {
            l.points = points;
            l.check_visibility = check_visibility;
            true
        }
        None => false,
    };
    if found {
        set(
            gm,
            PresKind::Lightning,
            id,
            "points",
            PropValue::Reals(points.to_vec()),
        );
    }
    found
}

/// One channel (0 r, 1 g, 2 b, 3 a) of a bolt's colour, as a real in 0..1 (UNSURE: the original
/// stores the bytes and reads them back through its conversion helpers).
fn lightning_channel(gm: &Game, id: u32, channel: usize) -> f32 {
    gm.presentation()
        .lightnings
        .get(id)
        .map_or(0.0, |l| l.color[channel] as f32 / 255.0)
}

/// Frees a pool object and logs it. `free` removes the entry and says whether there was one.
fn destroy_pool(
    gm: &mut Game,
    id: u32,
    kind: PresKind,
    free: impl FnOnce(&mut PresentationState) -> bool,
) {
    if free(gm.presentation_mut()) {
        log(gm, PresentationEvent::Destroyed { kind, handle: id });
    }
}

/// Sets a property of a pool object when `f` finds the live entry, and logs it.
fn pool_set(
    gm: &mut Game,
    id: u32,
    kind: PresKind,
    prop: &'static str,
    value: PropValue,
    f: impl FnOnce(&mut PresentationState) -> Option<()>,
) {
    if f(gm.presentation_mut()).is_some() {
        set(gm, kind, id, prop, value);
    }
}
