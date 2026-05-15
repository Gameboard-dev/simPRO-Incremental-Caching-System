
/// Fetch all pages from a simPRO list endpoint filtered by bounded `ID=in(...)`
/// filters.
/// The endpoint builder must accept the common `id`, `page`, `page_size`,
/// `columns`, `company_id`, and `send` calls used by generated `list` requests.
macro_rules! fetch_by_id {
    ($app:expr, $id_filters:expr, $getter:ident, $record:ty, $label:literal) => {{
        let mut records = Vec::new();

        for id_filter in $id_filters {
            let mut chunk_records = $crate::records::api::retrieval::paginate(|page| {
                let app = $app.clone();
                let id_filter = id_filter.clone();
                async move {
                    Ok(app
                        .api
                        .$getter()
                        .id(id_filter)
                        .page(page)
                        .page_size($crate::records::api::retrieval::PAGE_SIZE)
                        .columns(<$record as $crate::api::Columns>::COLUMNS.join(","))
                        .company_id(&app.company_id)
                        .send()
                        .await
                        .inspect_err(|err| tracing::error!(?err, concat!("Failed to fetch '", $label, "'")))?
                        .into_inner())
                }
            })
            .await?;

            records.append(&mut chunk_records);
        }

        records
    }};
}
pub(crate) use fetch_by_id;


/// Fetch all pages from a simPRO list endpoint filtered by `Date=between(...)`.
/// Intended for endpoints with the same generated builder shape as
/// [`fetch_records_by_id`], but where the search filter is applied through
/// the endpoint's `Date` parameter specified in `openapi.yaml`.
macro_rules! fetch_by_date {
    ($app:expr, $dates_between:expr, $getter:ident, $record:ty, $label:literal) => {{
        $crate::records::api::retrieval::paginate(|page| {
            let app = $app.clone();
            let dates_between = $dates_between.clone();
            async move {
                Ok(app
                    .api
                    .$getter()
                    .date(dates_between)
                    .page(page)
                    .page_size($crate::records::api::retrieval::PAGE_SIZE)
                    .columns(<$record as $crate::api::Columns>::COLUMNS.join(","))
                    .company_id(&app.company_id)
                    .send()
                    .await
                    .inspect_err(|err| tracing::error!(?err, concat!("Failed to fetch '", $label, "'")))?
                    .into_inner())
            }
        })
        .await?
    }};
}
pub(crate) use fetch_by_date;
