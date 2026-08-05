use crate::database::models::Template;
use crate::summary::templates::{self, defaults};
use anyhow::Result;
use sqlx::SqlitePool;

pub async fn sync_builtin_templates(pool: &SqlitePool) -> Result<()> {
    // Get all builtin template IDs
    let builtin_ids = defaults::list_builtin_template_ids();

    for id in builtin_ids {
        // A valid custom file with a built-in stable ID is the active source;
        // do not create a second database copy for it.
        if templates::template_origin(id).source == "custom" && templates::get_template(id).is_ok()
        {
            continue;
        }

        // Get template name and description from defaults
        let name = defaults::get_builtin_template_name(id).unwrap_or(id);
        let description = defaults::get_builtin_template_description(id).unwrap_or("");

        // Stable IDs are the source of truth. The name lookup only attaches an
        // un-migrated legacy row to its stable ID once; it is not used for
        // runtime deduplication.
        let existing = sqlx::query_as::<_, Template>(
            "SELECT * FROM templates WHERE stable_id = $1 AND is_builtin = 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        let existing = match existing {
            Some(template) => Some(template),
            None => sqlx::query_as::<_, Template>(
                "SELECT * FROM templates WHERE name = $1 AND is_builtin = 1 AND stable_id IS NULL",
            )
            .bind(name)
            .fetch_optional(pool)
            .await?,
        };

        if let Some(template) = existing {
            // Update schema_json if changed
            let content = defaults::get_builtin_template(id).unwrap_or("");
            if template.schema_json != content
                || template.name != name
                || template.stable_id.as_deref() != Some(id)
            {
                sqlx::query(
                    r#"
                    UPDATE templates
                    SET name = $1, description = $2, schema_json = $3, stable_id = $4, updated_at = $5
                    WHERE id = $6
                    "#,
                )
                .bind(name)
                .bind(description)
                .bind(content)
                .bind(id)
                .bind(chrono::Utc::now().to_rfc3339())
                .bind(template.id)
                .execute(pool)
                .await?;
            }
        } else {
            // Get the template content
            let content = defaults::get_builtin_template(id).unwrap_or("");

            // Insert into database
            sqlx::query(
                r#"
                INSERT INTO templates (name, description, stable_id, schema_json, is_builtin, created_at, updated_at)
                VALUES ($1, $2, $3, $4, 1, datetime('now'), datetime('now'))
                "#,
            )
            .bind(name)
            .bind(description)
            .bind(id)
            .bind(content)
            .execute(pool)
            .await?;
        }
    }

    Ok(())
}
