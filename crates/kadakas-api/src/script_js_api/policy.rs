//! What a script may call (SCR-08). Every method of the API is classified here, one by one, by
//! what it can touch. A method that is not in the table is **denied** until someone classifies it:
//! a test fails when `METHODS` has a method without a row, so a new method cannot slip into
//! the reach of scripts by accident.
//!
//! | Class | Meaning |
//! |---|---|
//! | `Read` | Reads the document, or computes and draws into memory. Changes nothing. |
//! | `Edit` | Changes the document (undoable), nothing else. |
//! | `Ui` | Changes the session but not the document: the selection, the view, the snapping. |
//! | `Files` | Reads or writes files, or can (a path in a parameter). |
//! | `Network` | Uses the network. |
//! | `Install` | Installs or removes plug-ins. |
//! | `Never` | A script never calls it: it would end the run, replace the document under it, change the session from outside, or run Python. |
//!
//! The code that an agent sends in `script.eval` keeps the policy of the session for files, the
//! network and plug-in installs (the path policy, the allowed addresses): it may call every class
//! except `Never`. In `mode: "read"` it may call `Read` methods only, and any other method is
//! refused **before** it runs.

/// What a method can touch. See the module text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Read,
    Edit,
    Ui,
    Files,
    Network,
    Install,
    Never,
}

use Class::*;

/// The class of each method, sorted by name.
const TABLE: &[(&str, Class)] = &[
    ("agent.activity", Read),
    ("annotation.apply_text_style", Edit),
    ("annotation.evaluate", Read),
    ("annotation.list", Read),
    ("annotation.text", Edit),
    ("annotation.update", Edit),
    ("bed.add", Edit),
    ("bed.get", Read),
    ("bed.list", Read),
    ("bed.set", Edit),
    ("callout.add", Edit),
    ("collab.connect", Never),
    ("collab.disconnect", Never),
    ("collab.presence", Never),
    ("collab.redo", Never),
    ("collab.status", Never),
    ("collab.sync", Never),
    ("collab.undo", Never),
    ("data_manager.apply", Edit),
    ("data_manager.check", Read),
    ("data_manager.create", Edit),
    ("data_manager.delete", Edit),
    ("data_manager.evaluate", Read),
    ("data_manager.get", Read),
    ("data_manager.list", Read),
    ("data_manager.properties", Read),
    ("data_manager.update", Edit),
    ("data_tag.add", Edit),
    ("data_tag.tag_all", Edit),
    ("describe", Read),
    ("dimension.add", Edit),
    ("dimension_style.create", Edit),
    ("dimension_style.delete", Edit),
    ("dimension_style.get", Read),
    ("dimension_style.list", Read),
    ("dimension_style.update", Edit),
    ("document.get", Read),
    ("document.load", Never),
    ("document.summary", Read),
    ("dxf.compare", Files),
    ("edit.array", Edit),
    ("edit.chamfer", Edit),
    ("edit.extend", Edit),
    ("edit.fillet", Edit),
    ("edit.mirror", Edit),
    ("edit.offset", Edit),
    ("edit.rotate", Edit),
    ("edit.scale", Edit),
    ("edit.split", Edit),
    ("edit.trim", Edit),
    ("events.poll", Never),
    ("events.subscribe", Never),
    ("events.unsubscribe", Never),
    ("execute", Edit),
    ("export.file", Files),
    ("extrude.add", Edit),
    ("extrude.from_shapes", Edit),
    ("extrude.get", Read),
    ("extrude.set", Edit),
    ("feedback.get", Never),
    ("feedback.list", Never),
    ("feedback.submit", Never),
    ("feedback.withdraw", Never),
    ("file.open", Never),
    ("file.save", Never),
    ("georef.survey_point", Edit),
    ("gis.cache.clear", Network),
    ("gis.coords", Network),
    ("gis.geoimage.import", Network),
    ("gis.georef.set", Network),
    ("gis.job.cancel", Network),
    ("gis.job.status", Network),
    ("gis.reproject", Network),
    ("gis.sources", Network),
    ("gis.wfs.capabilities", Network),
    ("gis.wfs.import", Network),
    ("gis.wms.add", Network),
    ("gis.wms.capabilities", Network),
    ("gis.wms.refresh", Network),
    ("gis.wmts.add", Network),
    ("gis.wmts.capabilities", Network),
    ("grade.add", Edit),
    ("grade.get", Read),
    ("grade.set", Edit),
    ("grade_limit.add", Edit),
    ("grid.set", Edit),
    ("group.context", Ui),
    ("group.copy", Edit),
    ("group.create", Edit),
    ("group.enter", Ui),
    ("group.exit", Ui),
    ("group.members", Edit),
    ("group.move", Edit),
    ("group.move_into", Edit),
    ("group.move_out", Edit),
    ("group.rotate", Edit),
    ("group.ungroup", Edit),
    ("hardscape.add", Edit),
    ("hardscape.course", Edit),
    ("hardscape.course_remove", Edit),
    ("hardscape.from_shapes", Edit),
    ("hardscape.plan", Edit),
    ("hardscape.plan_clear", Edit),
    ("hardscape.set", Edit),
    ("hardscape.split", Edit),
    ("hardscape.zone_set", Edit),
    ("hardscape_style.create", Edit),
    ("hardscape_style.delete", Edit),
    ("hardscape_style.duplicate", Edit),
    ("hardscape_style.get", Read),
    ("hardscape_style.list", Read),
    ("hardscape_style.update", Edit),
    ("import.apply", Files),
    ("import.file", Files),
    ("import.update", Files),
    ("irrigation.calculate", Edit),
    ("irrigation.catalog.export_csv", Read),
    ("irrigation.catalog.get", Read),
    ("irrigation.catalog.import_csv", Files),
    ("irrigation.catalog.set", Edit),
    ("irrigation.controller.add", Edit),
    ("irrigation.layout.drip", Edit),
    ("irrigation.layout.edge", Edit),
    ("irrigation.layout.fill", Edit),
    ("irrigation.line.add", Edit),
    ("irrigation.outlet.add", Edit),
    ("irrigation.poc.add", Edit),
    ("irrigation.report.hydrozones", Read),
    ("irrigation.report.materials", Read),
    ("irrigation.report.pipes", Read),
    ("irrigation.report.pressure", Read),
    ("irrigation.report.zones", Read),
    ("irrigation.set", Edit),
    ("irrigation.valve.add", Edit),
    ("issue.add_viewpoint", Edit),
    ("issue.comment", Edit),
    ("issue.create", Edit),
    ("issue.delete", Edit),
    ("issue.export_bcf", Files),
    ("issue.get", Read),
    ("issue.import_bcf", Files),
    ("issue.list", Read),
    ("issue.resolve", Edit),
    ("issue.update", Edit),
    ("landscape_area.add", Edit),
    ("landscape_area.set", Edit),
    ("landscape_area_style.create", Edit),
    ("landscape_area_style.delete", Edit),
    ("landscape_area_style.duplicate", Edit),
    ("landscape_area_style.get", Read),
    ("landscape_area_style.list", Read),
    ("landscape_area_style.update", Edit),
    ("legend.add", Edit),
    ("library.favorite_add", Files),
    ("library.favorite_remove", Files),
    ("library.favorites", Files),
    ("library.list", Files),
    ("massing.add", Edit),
    ("massing.follow_sources", Edit),
    ("massing.from_shapes", Edit),
    ("massing.get", Read),
    ("massing.report", Read),
    ("massing.set", Edit),
    ("material.add_defaults", Edit),
    ("material.assign", Edit),
    ("material.create", Edit),
    ("material.delete", Edit),
    ("material.duplicate", Edit),
    ("material.get", Read),
    ("material.import_texture", Files),
    ("material.list", Read),
    ("material.slots", Read),
    ("material.update", Edit),
    ("mesh.add_curve", Edit),
    ("mesh.add_edges", Edit),
    ("mesh.batch", Edit),
    ("mesh.bounds", Read),
    ("mesh.copy", Edit),
    ("mesh.create", Edit),
    ("mesh.edges", Read),
    ("mesh.erase", Edit),
    ("mesh.explode", Edit),
    ("mesh.faces", Read),
    ("mesh.follow_me", Edit),
    ("mesh.hide", Edit),
    ("mesh.intersect_faces", Edit),
    ("mesh.make_component", Edit),
    ("mesh.make_group", Edit),
    ("mesh.near", Read),
    ("mesh.offset", Edit),
    ("mesh.paint", Edit),
    ("mesh.pick", Read),
    ("mesh.push_pull", Edit),
    ("mesh.render", Read),
    ("mesh.reverse_faces", Edit),
    ("mesh.soften", Edit),
    ("mesh.soften_edges", Edit),
    ("mesh.solid", Edit),
    ("mesh.solid_report", Read),
    ("mesh.summary", Read),
    ("mesh.transform", Edit),
    ("mesh.vertices", Read),
    ("north_arrow.add", Edit),
    ("notes_db.create", Edit),
    ("notes_db.delete", Edit),
    ("notes_db.get", Read),
    ("notes_db.list", Read),
    ("notes_db.update", Edit),
    ("objects.get", Read),
    ("objects.list", Read),
    ("objects.query", Read),
    ("pad.add", Edit),
    ("pad.retaining_edge", Edit),
    ("pattern.segments", Read),
    ("phase.display", Edit),
    ("phase.report", Read),
    ("phase.set", Edit),
    ("plant_style.clear_picture", Edit),
    ("plant_style.create", Edit),
    ("plant_style.delete", Edit),
    ("plant_style.duplicate", Edit),
    ("plant_style.export_csv", Read),
    ("plant_style.get", Read),
    ("plant_style.get_picture", Read),
    ("plant_style.import_csv", Files),
    ("plant_style.list", Read),
    ("plant_style.sample_csv", Read),
    ("plant_style.set_picture", Files),
    ("plant_style.update", Edit),
    ("plant_tag.set", Edit),
    ("plants.drape", Edit),
    ("plants.place", Edit),
    ("plants.set_style", Edit),
    ("plugin.define", Never),
    ("plugin.delete", Never),
    ("plugin.examples", Never),
    ("plugin.get", Never),
    ("plugin.install_example", Never),
    ("plugin.list", Never),
    ("plugin.object", Never),
    ("plugin.place", Never),
    ("plugin.preview", Never),
    ("plugin.reset", Never),
    ("plugin.set_params", Never),
    ("plugin.update", Never),
    ("plugins.info", Read),
    ("plugins.install", Install),
    ("plugins.list", Read),
    ("plugins.object", Read),
    ("plugins.place", Edit),
    ("plugins.preview", Edit),
    ("plugins.remove", Install),
    ("plugins.reset", Edit),
    ("plugins.set_params", Edit),
    ("plugins.tool_step", Edit),
    ("polygon3d.add", Edit),
    ("polygon3d.get", Read),
    ("polygon3d.set", Edit),
    ("project.get", Read),
    ("project.new_guid", Edit),
    ("project.set", Edit),
    ("record.attach", Edit),
    ("record.detach", Edit),
    ("record.get", Read),
    ("record.set", Edit),
    ("record_format.create", Edit),
    ("record_format.delete", Edit),
    ("record_format.get", Read),
    ("record_format.list", Read),
    ("record_format.update", Edit),
    ("redo", Never),
    ("region.from_curves", Edit),
    ("render.hidden_line", Read),
    ("render.png", Read),
    ("render.svg", Read),
    ("render_style.create", Edit),
    ("render_style.delete", Edit),
    ("render_style.duplicate", Edit),
    ("render_style.get", Read),
    ("render_style.list", Read),
    ("render_style.set_active", Ui),
    ("render_style.update", Edit),
    ("report.areas", Read),
    ("report.existing_trees", Read),
    ("report.hardscape", Read),
    ("report.landscape_areas", Read),
    ("report.plant_list", Read),
    ("report.plant_schedule", Read),
    ("report.plants", Read),
    ("report.unit_pattern", Read),
    ("resource.create", Edit),
    ("resource.delete", Edit),
    ("resource.duplicate", Edit),
    ("resource.export_to_file", Files),
    ("resource.get", Read),
    ("resource.import_from_file", Files),
    ("resource.list", Read),
    ("resource.list_file", Files),
    ("resource.purge_unused", Edit),
    ("resource.rename", Edit),
    ("resource.thumbnail", Read),
    ("resource.update", Edit),
    ("revision.list", Read),
    ("revision_cloud.add", Edit),
    ("saved_view.delete", Edit),
    ("saved_view.list", Read),
    ("saved_view.move", Edit),
    ("saved_view.rename", Edit),
    ("saved_view.restore", Ui),
    ("saved_view.save", Edit),
    ("scale_bar.add", Edit),
    ("script.eval", Never),
    ("script.info", Never),
    ("script.js_info", Read),
    ("script.list", Never),
    ("script.run", Never),
    ("script.status", Never),
    ("script.stop", Never),
    ("section.add", Edit),
    ("section.place", Edit),
    ("section.profile", Read),
    ("section.set", Edit),
    ("section_viewport.create", Edit),
    ("section_viewport.get", Read),
    ("section_viewport.line_add", Edit),
    ("section_viewport.line_set", Edit),
    ("section_viewport.update", Edit),
    ("selection.click", Ui),
    ("selection.get", Read),
    ("selection.set", Ui),
    ("sheet.create", Edit),
    ("sheet.list", Read),
    ("sheet.render", Read),
    ("sheet.set", Edit),
    ("site.analysis", Edit),
    ("site.check", Read),
    ("site.contours", Edit),
    ("site.create", Edit),
    ("site.cut_fill", Read),
    ("site.drape", Edit),
    ("site.elevation", Read),
    ("site.refresh", Edit),
    ("site.simplify", Edit),
    ("site.texture", Edit),
    ("site.update", Edit),
    ("snap.get", Read),
    ("snap.query", Read),
    ("snap.set", Ui),
    ("snap.smart", Ui),
    ("spot.add", Edit),
    ("spot.get", Read),
    ("spot.set", Edit),
    ("sun.get", Read),
    ("sun.set", Edit),
    ("surface.add", Edit),
    ("surface.clip", Edit),
    ("surface.compose", Edit),
    ("surface.intersect", Edit),
    ("surface.subtract", Edit),
    ("survey.import_points", Files),
    ("sweep.add", Edit),
    ("sweep.from_shapes", Edit),
    ("sweep.get", Read),
    ("sweep.set", Edit),
    ("symbol.convert_to_group", Edit),
    ("symbol.create", Edit),
    ("symbol.edit", Edit),
    ("symbol.edit_end", Edit),
    ("symbol.from_objects", Edit),
    ("symbol.get", Read),
    ("symbol.list", Read),
    ("symbol.place", Edit),
    ("symbol.set", Edit),
    ("tag_style.create", Edit),
    ("tag_style.delete", Edit),
    ("tag_style.get", Read),
    ("tag_style.list", Read),
    ("tag_style.update", Edit),
    ("text_style.create", Edit),
    ("text_style.delete", Edit),
    ("text_style.get", Read),
    ("text_style.list", Read),
    ("text_style.update", Edit),
    ("title_block.create", Edit),
    ("title_block.resolve", Read),
    ("title_block.update", Edit),
    ("tree.import_survey", Files),
    ("undo", Never),
    ("unit_pattern.check", Read),
    ("unit_pattern.layout", Read),
    ("unit_pattern.module_from_block", Files),
    ("unit_pattern.module_from_shapes", Read),
    ("unit_pattern.module_preset", Read),
    ("unit_pattern.to_objects", Edit),
    ("version.create", Edit),
    ("version.delete", Edit),
    ("version.diff", Read),
    ("version.get", Read),
    ("version.list", Read),
    ("version.repair", Edit),
    ("version.restore", Edit),
    ("view.camera", Ui),
    ("view.get", Read),
    ("view.highlight", Ui),
    ("view.highlight_clear", Ui),
    ("view.list", Read),
    ("view.pan", Ui),
    ("view.screenshot", Read),
    ("view.set", Ui),
    ("view.zoom", Ui),
    ("view_rules.get", Read),
    ("viewport.annotate", Edit),
    ("viewport.create", Edit),
    ("viewport.get", Read),
    ("viewport.list", Read),
    ("viewport.set_override", Edit),
    ("viewport.update", Edit),
    ("visibility.get", Read),
    ("visibility.set", Edit),
    ("visualization.auto", Edit),
    ("visualization.clear", Edit),
    ("visualization.get", Read),
    ("visualization.set", Edit),
    ("wall.add", Edit),
    ("wall.report", Read),
    ("wall.set", Edit),
    ("wall_style.create", Edit),
    ("wall_style.delete", Edit),
    ("wall_style.list", Read),
    ("wall_style.update", Edit),
    ("worksheet.create", Edit),
    ("worksheet.delete", Edit),
    ("worksheet.evaluate", Read),
    ("worksheet.export", Files),
    ("worksheet.fit", Edit),
    ("worksheet.get", Read),
    ("worksheet.list", Read),
    ("worksheet.place", Edit),
    ("worksheet.remove_database", Edit),
    ("worksheet.rename", Edit),
    ("worksheet.set_cell", Edit),
    ("worksheet.set_cells", Edit),
    ("worksheet.set_database", Edit),
    ("worksheet.templates", Read),
];

/// The class of a method, or `None` if nobody classified it.
pub fn class_of(method: &str) -> Option<Class> {
    TABLE
        .binary_search_by_key(&method, |(m, _)| m)
        .ok()
        .map(|i| TABLE[i].1)
}

/// Every method that has a row (for the tests).
#[cfg(test)]
pub fn classified() -> impl Iterator<Item = &'static str> {
    TABLE.iter().map(|(m, _)| *m)
}

/// Why a script may not call this method in this mode, or `Ok` if it may.
pub fn check(method: &str, read_only: bool) -> Result<(), String> {
    let Some(class) = class_of(method) else {
        return Err(format!(
            "{method} is not classified for scripts, so a script cannot call it (the table is in script_js_api/policy.rs)"
        ));
    };
    match class {
        Never => Err(format!(
            "a script cannot call {method}: it would end the run, replace the document under it, change the session from outside or run Python"
        )),
        Read => Ok(()),
        _ if read_only => Err(format!(
            "{method} is not a read-only method, so a script in mode read cannot call it"
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_and_every_method_has_a_row_and_every_row_a_method() {
        assert!(
            TABLE.windows(2).all(|w| w[0].0 < w[1].0),
            "sorted, no doubles"
        );
        let methods: Vec<&str> = crate::METHODS.iter().map(|(m, _)| *m).collect();
        for m in &methods {
            assert!(class_of(m).is_some(), "{m} has no row in the policy table");
        }
        // The rows of methods that the command line program answers itself.
        let host_methods = ["file.open", "file.save"];
        for m in classified() {
            assert!(
                methods.contains(&m) || host_methods.contains(&m),
                "{m} is in the table but is not a method"
            );
        }
    }

    #[test]
    fn read_mode_is_an_allow_list_and_never_methods_are_never_allowed() {
        assert!(check("objects.list", true).is_ok());
        assert!(check("report.unit_pattern", true).is_ok());
        assert!(check("execute", true).is_err());
        assert!(check("export.file", true).is_err());
        assert!(check("execute", false).is_ok());
        assert!(
            check("export.file", false).is_ok(),
            "the session decides about files"
        );
        assert!(check("gis.wms.add", false).is_ok());
        assert!(check("plugins.install", false).is_ok());
        for m in [
            "script.eval",
            "script.run",
            "plugin.place",
            "plugin.reset",
            "undo",
            "redo",
            "document.load",
            "collab.connect",
            "events.poll",
        ] {
            assert!(check(m, false).is_err(), "{m}");
            assert!(check(m, true).is_err(), "{m}");
        }
        assert!(
            check("no.such.method", false).is_err(),
            "unclassified is denied"
        );
    }

    #[test]
    fn the_read_class_has_no_method_that_changes_things() {
        // A rough guard on the names: these words mean a change.
        for m in classified().filter(|m| class_of(m) == Some(Read)) {
            for word in [
                "add", "set", "create", "delete", "update", "place", "import", "apply", "remove",
                "restore", "save", "move", "reset",
            ] {
                let last = m.rsplit('.').next().unwrap_or(m);
                assert!(
                    !last.split('_').any(|w| w == word),
                    "{m} is classified Read but its name says '{word}'"
                );
            }
        }
    }
}
