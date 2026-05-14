
/// The maximum number of results to be returned by a request (`integer [1...1000]`)
pub(crate) const PAGE_SIZE: i64 = 1000;

/// simPRO list endpoints return at most [`PAGE_SIZE`] records per request,
/// so a single API call may not contain the complete result set. 
/// 
/// This helper keeps requesting pages until the endpoint returns fewer than [`PAGE_SIZE`] 
/// records, which indicates that the final page has been reached.
///
/// Each returned page is appended into a single `Vec<T>`, so callers receive a
/// flattened result.
/// 
/// This keeps pagination centralized and makes endpoint-specific code only
/// responsible for describing how to fetch one page.
/// 
/// ### Documentation:
/// https://developer.simprogroup.com/apidoc/?page=ccdb7bf9d93e5652b57cabcc8c41e061#tag/Schedules/operation/c81549288cc61e04c339b32a65425326
pub(crate) async fn paginate<T, Fut, F>(mut fetch_page: F) -> anyhow::Result<Vec<T>>
where
    F: FnMut(i64) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<T>>>,
{
    let mut page = 1;
    let mut all = Vec::new();

    loop {
        let mut records = fetch_page(page).await?;
        let count = records.len();

        all.append(&mut records);

        if count < PAGE_SIZE as usize {
            break;
        }

        page += 1;
    }

    Ok(all)
}