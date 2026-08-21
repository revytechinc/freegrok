#![cfg_attr(rustfmt, rustfmt::skip)]
    use super::*;

    /// Stale or duplicate gens short-circuit BEFORE the seam (and its config
    /// disk loads) runs — nothing observable may change.
    #[test]
    fn announcements_update_stale_gen_short_circuits_before_apply() {
        let mut app = make_app_with_agent("sess-ann");
        app.announcements_last_gen = 5;
        app.active_announcements = vec![critical_announcement("current")];
        // Marker the seam cannot leave intact: it matches no pushed
        // announcement, so any apply would prune it (and queue a persist).
        app.hidden_announcement_ids = ["stale-key".to_string()].into_iter().collect();

        for stale_gen in [4, 5] {
            let changed = handle_ext_notification(
                &announcements_update_notif(stale_gen, &[critical_announcement("stale-push")]),
                &mut app,
            );
            assert!(!changed, "gen {stale_gen} must be dropped at the watermark");
        }

        assert_eq!(
            app.active_announcements,
            vec![critical_announcement("current")]
        );
        assert_eq!(app.announcements_last_gen, 5);
        assert!(app.hidden_announcement_ids.contains("stale-key"));
        assert!(
            !app.pending_effects
                .iter()
                .any(|e| matches!(e, Effect::PersistAnnouncementsHidden { .. })),
            "short-circuit must not reach prune/persist, got {:?}",
            app.pending_effects
        );
    }

    /// The watermark is connection-scoped: the event loop's leader-reconnected
    /// branch resets it to 0, so a re-elected shell's fresh (possibly lower)
    /// gen sequence applies, and a duplicate broadcast seed copy stays a no-op.
    #[test]
    fn announcements_update_applies_after_reconnect_watermark_reset() {
        let mut app = make_app_with_agent("sess-ann");
        // Watermark from the previous connection, ahead of the new shell's gens.
        app.announcements_last_gen = 9_999_999_999;
        // The leader-reconnected branch's connection-scoped reset.
        app.announcements_last_gen = 0;

        let first = handle_ext_notification(
            &announcements_update_notif(1, &[critical_announcement("fresh")]),
            &mut app,
        );
        assert!(first, "gen 1 must apply after the reconnect reset");
        assert_eq!(app.announcements_last_gen, 1);
        assert!(
            !app.active_announcements
                .iter()
                .any(|a| a.id.as_deref() == Some("fresh")),
            "FreeGrok must ignore remote xAI announcements (id=fresh)"
        );

        // The per-client seed broadcast can deliver the same gen twice.
        let dup = handle_ext_notification(
            &announcements_update_notif(1, &[critical_announcement("fresh")]),
            &mut app,
        );
        assert!(!dup, "duplicate seed copy must be idempotent");
        assert_eq!(app.announcements_last_gen, 1);
    }

    /// A push prunes hidden ids whose announcement is gone and schedules a
    /// persist so the on-disk set cannot grow unboundedly. Driven through the
    /// layer-injected seam so the developer's real `~/.grok` cannot leak in.
    #[test]
    fn announcements_update_prunes_stale_hidden_ids_and_persists() {
        let mut app = make_app_with_agent("sess-ann");
        app.hidden_announcement_ids = ["gone".to_string(), "live".to_string()]
            .into_iter()
            .collect();

        let user_cfg: toml::Value = toml::from_str(
            r#"
            [[announcements]]
            id = "live"
            title = "Local outage"
            message = "from user config"
            severity = "critical"
            "#,
        )
        .unwrap();
        apply_announcements_update(
            &mut app,
            1,
            &[critical_announcement("live")],
            None,
            Some(&user_cfg),
            None,
        );

        assert_eq!(app.announcements_last_gen, 1);
        let expected: std::collections::BTreeSet<String> =
            ["live".to_string()].into_iter().collect();
        assert_eq!(app.hidden_announcement_ids, expected);
        assert!(
            app.pending_effects.iter().any(|e| matches!(
                e,
                Effect::PersistAnnouncementsHidden { hidden_ids } if hidden_ids == &expected
            )),
            "prune must persist the shrunken set, got {:?}",
            app.pending_effects
        );
        assert_eq!(
            shown_banner_id(&app),
            None,
            "surviving hidden id still hides its banner"
        );
    }

    /// A pushed critical with a NEW id must re-arm the banner even though an
    /// older critical was hidden (the whole point of per-ID hide). Driven
    /// through the layer-injected seam (no real `~/.grok` reads).
    #[test]
    fn announcements_update_new_critical_id_rearms_hidden_banner() {
        let mut app = make_app_with_agent("sess-ann");
        let cfg_a: toml::Value = toml::from_str(
            r#"
            [[announcements]]
            id = "outage-a"
            title = "Outage A"
            message = "from user config"
            severity = "critical"
            "#,
        )
        .unwrap();
        apply_announcements_update(&mut app, 1, &[], None, Some(&cfg_a), None);
        app.hidden_announcement_ids.insert("outage-a".to_string());
        assert_eq!(shown_banner_id(&app), None);

        let cfg_b: toml::Value = toml::from_str(
            r#"
            [[announcements]]
            id = "outage-b"
            title = "Outage B"
            message = "from user config"
            severity = "critical"
            "#,
        )
        .unwrap();
        apply_announcements_update(&mut app, 2, &[], None, Some(&cfg_b), None);

        assert_eq!(app.announcements_last_gen, 2);
        assert_eq!(
            shown_banner_id(&app).as_deref(),
            Some("outage-b"),
            "new critical id must re-show the banner"
        );
        // The stale hide key was pruned with the list replacement.
        assert!(app.hidden_announcement_ids.is_empty());
    }

    /// A push must not drop config-layer announcements, and prune must not
    /// erase their persisted hide keys — they re-resolve every launch and a
    /// dropped key would re-show a critical the user already hid.
    #[test]
    fn announcements_update_remerges_config_layers_and_keeps_their_hide_keys() {
        let mut app = make_app_with_agent("sess-ann");
        let user_cfg: toml::Value = toml::from_str(
            r#"
            [[announcements]]
            id = "cfg-crit"
            title = "Config outage"
            message = "from user config"
            severity = "critical"
            "#,
        )
        .unwrap();
        app.hidden_announcement_ids = ["cfg-crit".to_string()].into_iter().collect();

        apply_announcements_update(
            &mut app,
            1,
            &[critical_announcement("live")],
            None,
            Some(&user_cfg),
            None,
        );

        assert_eq!(app.announcements_last_gen, 1);
        let ids: Vec<_> = app
            .active_announcements
            .iter()
            .filter_map(|a| a.id.as_deref())
            .collect();
        assert_eq!(
            ids,
            ["cfg-crit"],
            "config-layer announcement must survive; remote xAI id=live is ignored"
        );
        assert!(
            app.hidden_announcement_ids.contains("cfg-crit"),
            "config-layer hide key must survive prune"
        );
        assert!(
            !app.pending_effects
                .iter()
                .any(|e| matches!(e, Effect::PersistAnnouncementsHidden { .. })),
            "unchanged hidden set must not schedule a persist, got {:?}",
            app.pending_effects
        );
        assert_eq!(
            shown_banner_id(&app),
            None,
            "remote push ignored; hidden config-layer critical stays skipped"
        );
    }

    /// xAI `/v1/settings` + ACP `announcements/update` marketing must not
    /// paint in FreeGrok (the "Grok 4.5 is here" welcome ad).
    #[test]
    fn remote_xai_marketing_announcement_is_ignored() {
        let mut app = make_app_with_agent("sess-ann");
        let mut promo = critical_announcement("promo");
        promo.title = Some("Grok 4.5 is here. Upgrade now.".into());
        promo.message = Some("Upgrade now.".into());
        apply_announcements_update(&mut app, 1, &[promo], None, None, None);
        assert!(
            app.active_announcements.is_empty(),
            "remote marketing must not reach welcome: {:?}",
            app.active_announcements
        );
        assert_eq!(shown_banner_id(&app), None);
    }

    /// A mid-session push must open the `/announcements` gate on already-live
    /// subagent child views, not just top-level agents. Driven through the
    /// layer-injected seam (no real `~/.grok` reads).
    #[test]
    fn announcements_update_fans_slash_gate_to_live_subagent_views() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");
        assert!(
            !app.agents[&AgentId(0)].subagent_views["child-sess"]
                .prompt
                .slash_controller
                .has_session_announcements(),
            "gate starts closed"
        );

        let user_cfg: toml::Value = toml::from_str(
            r#"
            [[announcements]]
            id = "outage-a"
            title = "Outage"
            message = "from user config"
            severity = "critical"
            "#,
        )
        .unwrap();
        apply_announcements_update(&mut app, 1, &[], None, Some(&user_cfg), None);

        let agent = &app.agents[&AgentId(0)];
        assert!(
            agent.prompt.slash_controller.has_session_announcements(),
            "parent gate open"
        );
        assert!(
            agent.subagent_views["child-sess"]
                .prompt
                .slash_controller
                .has_session_announcements(),
            "live child view gate open"
        );
    }

