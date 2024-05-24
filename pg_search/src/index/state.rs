use super::score::SearchIndexScore;
use super::SearchIndex;
use crate::schema::{SearchConfig, SearchFieldName, SearchIndexSchema};
use derive_more::{AsRef, Display, From};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use shared::postgres::transaction::{Transaction, TransactionError};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use tantivy::collector::TopDocs;
use tantivy::schema::FieldType;
use tantivy::{query::Query, DocAddress, Score, Searcher};
use tantivy::{Executor, Snippet, SnippetGenerator};
use thiserror::Error;

static SEARCH_STATE_MANAGER: Lazy<Arc<Mutex<SearchStateManager>>> = Lazy::new(|| {
    Arc::new(Mutex::new(SearchStateManager {
        state_map: HashMap::new(),
        result_map: HashMap::new(),
    }))
});

const TRANSACTION_CALLBACK_CACHE_ID: &str = "parade_current_search";

pub struct SearchStateManager {
    state_map: HashMap<SearchAlias, SearchState>,
    result_map: HashMap<SearchAlias, HashMap<String, (Score, DocAddress)>>,
}

impl SearchStateManager {
    fn register_callback() -> Result<(), TransactionError> {
        // Commit and abort are mutually exclusive. One of the two is guaranteed
        // to be called on any transaction. We'll use that opportunity to clean
        // up the cache.
        Transaction::call_once_on_commit(TRANSACTION_CALLBACK_CACHE_ID, move || {
            let mut current_search = SEARCH_STATE_MANAGER
                .lock()
                .expect("could not lock current search lookup in commit callback");
            current_search.state_map.drain();
        })?;
        Transaction::call_once_on_abort(TRANSACTION_CALLBACK_CACHE_ID, move || {
            let mut current_search = SEARCH_STATE_MANAGER
                .lock()
                .expect("could not lock current search lookup in abort callback");
            current_search.state_map.drain();
        })?;
        Ok(())
    }

    fn get_state_default(&self) -> Result<&SearchState, SearchStateError> {
        self.state_map
            .get(&SearchAlias::default())
            .ok_or(SearchStateError::NoQuery)
    }

    fn get_state_alias(&self, alias: SearchAlias) -> Result<&SearchState, SearchStateError> {
        self.state_map
            .get(&alias)
            .ok_or(SearchStateError::AliasLookup(alias))
    }

    pub fn get_score(key: String, alias: Option<SearchAlias>) -> Result<Score, SearchStateError> {
        let manager = SEARCH_STATE_MANAGER
            .lock()
            .map_err(SearchStateError::from)?;
        let (score, _) = manager
            .result_map
            .get(&alias.unwrap_or_default())
            .and_then(|inner_map| inner_map.get(&key))
            .ok_or(SearchStateError::DocLookup(key))?;

        Ok(*score)
    }

    pub fn get_snippet(
        key: String,
        field_name: &str,
        max_num_chars: Option<usize>,
        alias: Option<SearchAlias>,
    ) -> Result<Snippet, SearchStateError> {
        let manager = SEARCH_STATE_MANAGER
            .lock()
            .map_err(SearchStateError::from)?;
        let state = manager.get_state(alias.clone())?;
        let mut snippet_generator = state.snippet_generator(field_name);
        if let Some(max_num_chars) = max_num_chars {
            snippet_generator.set_max_num_chars(max_num_chars)
        }

        let alias = alias.unwrap_or_default();

        let (_, doc_address) = manager
            .result_map
            .get(&alias)
            .and_then(|inner_map| inner_map.get(&key))
            .ok_or(SearchStateError::DocLookup(key))?;
        let doc = state
            .searcher
            .doc(*doc_address)
            .expect("could not find document in searcher");
        Ok(snippet_generator.snippet_from_doc(&doc))
    }

    pub fn get_state(&self, alias: Option<SearchAlias>) -> Result<&SearchState, SearchStateError> {
        if let Some(alias) = alias {
            self.get_state_alias(alias)
        } else {
            self.get_state_default()
        }
    }

    fn set_state_default(&mut self, state: SearchState) -> Result<(), SearchStateError> {
        match self.state_map.insert(SearchAlias::default(), state) {
            None => Ok(()),
            Some(_) => Err(SearchStateError::AliasRequired),
        }
    }

    fn set_state_alias(
        &mut self,
        state: SearchState,
        alias: SearchAlias,
    ) -> Result<(), SearchStateError> {
        if alias == SearchAlias::default() {
            Err(SearchStateError::EmptyAlias)
        } else {
            if self.state_map.insert(alias.clone(), state).is_some() {
                return Err(SearchStateError::DuplicateAlias(alias));
            }
            Ok(())
        }
    }

    pub fn set_state(state: SearchState) -> Result<(), SearchStateError> {
        Self::register_callback().map_err(SearchStateError::from)?;

        let mut manager = SEARCH_STATE_MANAGER
            .lock()
            .map_err(SearchStateError::from)?;
        if let Some(ref alias) = state.config.alias {
            let alias = alias.clone();
            manager.set_state_alias(state, alias)
        } else {
            manager.set_state_default(state)
        }
    }

    pub fn set_result(
        key: String,
        score: Score,
        doc_address: DocAddress,
        alias: Option<SearchAlias>,
    ) -> Result<(), SearchStateError> {
        let mut manager = SEARCH_STATE_MANAGER
            .lock()
            .map_err(SearchStateError::from)?;

        manager
            .result_map
            .entry(alias.unwrap_or_default())
            .or_insert_with(HashMap::new)
            .insert(key, (score, doc_address));
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum SearchStateError {
    #[error("to use multiple pg_search queries, pass a query alias with the 'as' parameter")]
    AliasRequired,
    #[error("no pg_search query in current transaction")]
    NoQuery,
    #[error("a pg_search alias string cannot be empty")]
    EmptyAlias,
    #[error("a pg_search alias must be unique, found duplicate: '{0}'")]
    DuplicateAlias(SearchAlias),
    #[error("error looking up result data for document with id: '{0}'")]
    DocLookup(String),
    #[error("no query found with alias: '{0}'")]
    AliasLookup(SearchAlias),
    #[error("could not lock the current search config lookup: {0}")]
    Lock(String),
    #[error("could not register callback for search state manager: {0}")]
    CallbackError(#[from] TransactionError),
}

impl<T> From<PoisonError<T>> for SearchStateError {
    fn from(err: PoisonError<T>) -> Self {
        SearchStateError::Lock(format!("{err}"))
    }
}

#[derive(Clone, Debug, Display, AsRef, Eq, PartialEq, Hash, From, Deserialize, Serialize)]
#[as_ref(forward)]
pub struct SearchAlias(String);

impl From<&str> for SearchAlias {
    fn from(value: &str) -> Self {
        SearchAlias(value.to_string())
    }
}

impl Default for SearchAlias {
    fn default() -> Self {
        SearchAlias("".into())
    }
}

#[derive(Clone)]
pub struct SearchState {
    pub query: Arc<dyn Query>,
    pub searcher: Searcher,
    pub config: SearchConfig,
    pub schema: SearchIndexSchema,
}

impl SearchState {
    pub fn new(search_index: &SearchIndex, config: &SearchConfig) -> Self {
        let schema = search_index.schema.clone();
        let mut parser = search_index.query_parser();
        let query = config
            .query
            .clone()
            .into_tantivy_query(&schema, &mut parser)
            .expect("could not parse query");
        SearchState {
            query: Arc::new(query),
            config: config.clone(),
            searcher: search_index.searcher(),
            schema: schema.clone(),
        }
    }

    pub fn snippet_generator(&self, field_name: &str) -> SnippetGenerator {
        let field = self
            .schema
            .get_search_field(&SearchFieldName(field_name.into()))
            .expect("cannot generate snippet, field does not exist");

        match self.schema.schema.get_field_entry(field.into()).field_type() {
            FieldType::Str(_) => {
                SnippetGenerator::create(&self.searcher, self.query.as_ref(), field.into())
                    .unwrap_or_else(|err| panic!("failed to create snippet generator for field: {field_name}... {err}"))
            },
            _ => panic!("failed to create snippet generator for field: {field_name}... can only highlight text fields")
        }
    }

    /// Search the Tantivy index for matching documents. If used outside of Postgres
    /// index access methods, this may return deleted rows until a VACUUM. If you need to scan
    /// the Tantivy index without a Postgres deduplication, you should use the `search_dedup`
    /// method instead.
    pub fn search(&self, executor: &Executor) -> Vec<(Score, DocAddress, String, u64)> {
        // Extract limit and offset from the query config or set defaults.
        let limit = self.config.limit_rows.unwrap_or_else(|| {
            // We use unwrap_or_else here so this block doesn't run unless
            // we actually need the default value. This is important, because there can
            // be some cost to Tantivy API calls.
            let num_docs = self.searcher.num_docs() as usize;
            if num_docs > 0 {
                num_docs // The collector will panic if it's passed a limit of 0.
            } else {
                1 // Since there's no docs to return anyways, just use 1.
            }
        });

        let offset = self.config.offset_rows.unwrap_or(0);

        if self.config.stable_sort.is_some_and(|stable| stable) {
            // If the user requires a stable sort, we'll use tweak_score. This allows us to retrieve
            // the value of a fast field and use that as a secondary sort key. In the case of a
            // bm25 score tie, results will be ordered based on the value of their 'key_field'.
            // This has a big performance impact, so the user needs to opt-in.
            let key_field_name = self.config.key_field.clone();
            let collector = TopDocs::with_limit(limit).and_offset(offset).tweak_score(
                move |segment_reader: &tantivy::SegmentReader| {
                    let fast_fields = segment_reader
                        .fast_fields();

                    if fast_fields.column_num_bytes(&key_field_name).unwrap() == 0 {
                        panic!("0!!!");
                    }

                    let key_field_reader = fast_fields
                        .i64(&key_field_name)
                        .unwrap_or_else(|err| panic!("key field {} is not a i64: {err:?}", "id"))
                        .first_or_default_col(0);
                    // let key = fast_fields
                    //     .i64(&key_field_name)
                    //     .map_or_else(|_| {
                    //         fast_fields.str(&key_field_name)
                    //         .map_or_else(|_| {
                    //             fast_fields.f64(&key_field_name)
                    //             .map_or_else(|_| {
                    //                 fast_fields.u64(&key_field_name)
                    //                 .map_or_else(|_| {
                    //                     fast_fields.date(&key_field_name)
                    //                     .map_or_else(|_| {
                    //                         panic!("key field not a fast field")
                    //                     }, |i| format!("{}", i.first_or_default_col(tantivy::DateTime::MIN).get_val(doc)))
                    //                 }, |i| format!("{}", i.first_or_default_col(0).get_val(doc)))
                    //             }, |i| format!("{}", i.first_or_default_col(0.0).get_val(doc)))
                    //         }, |i| {
                    //             let mut ret_str: String;
                    //             i.ord_to_str(0, ret_str);
                    //             ret_str
                    //         })
                    //     }, |i| format!("{}", i.first_or_default_col(0).get_val(doc)));
                    // TODO: get the value and turn into a string

                    // This function will be called on every document in the index that matches the
                    // query, before limit + offset are applied. It's important that it's efficient.
                    move |doc: tantivy::DocId, original_score: tantivy::Score| {
                        // let value = doc
                        //     .get_first(key_field_name)
                        //     .unwrap();

                        // let key = match value {
                        //     tantivy::schema::Value::Str(string) => string.clone(),
                        //     tantivy::schema::Value::U64(u64) => format!("{:?}", u64),
                        //     tantivy::schema::Value::I64(i64) => format!("{:?}", i64),
                        //     tantivy::schema::Value::F64(f64) => format!("{:?}", f64),
                        //     tantivy::schema::Value::Bool(bool) => format!("{:?}", bool),
                        //     tantivy::schema::Value::Date(datetime) => datetime.into_primitive().to_string(),
                        //     tantivy::schema::Value::Bytes(bytes) => String::from_utf8(bytes.clone()).unwrap(),
                        //     _ => panic!("NO")
                        // };

                        // let key = fast_fields
                        // .i64(&key_field_name)
                        // .map_or_else(|_| {
                        //     fast_fields.str(&key_field_name)
                        //     .map_or_else(|_| {
                        //         fast_fields.f64(&key_field_name)
                        //         .map_or_else(|_| {
                        //             fast_fields.u64(&key_field_name)
                        //             .map_or_else(|_| {
                        //                 fast_fields.date(&key_field_name)
                        //                 .map_or_else(|_| {
                        //                     panic!("key field not a fast field")
                        //                 }, |i| format!("{}", i.first_or_default_col(tantivy::DateTime::MIN).get_val(doc).into_primitive().to_string()))
                        //             }, |i| format!("{}", i.first_or_default_col(0).get_val(doc)))
                        //         }, |i| format!("{}", i.first_or_default_col(0.0).get_val(doc)))
                        //     }, |i| {
                        //         let mut ret_str: String;
                        //         i.unwrap().ord_to_str(0, &mut ret_str);
                        //         ret_str
                        //     })
                        // }, |i| format!("{}", i.first_or_default_col(0).get_val(doc)));

                        SearchIndexScore {
                            bm25: original_score,
                            key: format!("{}", key_field_reader.get_val(doc)),
                            // key: key
                        }
                    }
                },
            );
            self.searcher
                .search_with_executor(
                    self.query.as_ref(),
                    &collector,
                    executor,
                    tantivy::query::EnableScoring::Enabled {
                        searcher: &self.searcher,
                        statistics_provider: &self.searcher,
                    },
                )
                .expect("failed to search")
                .into_iter()
                .map(|(score, doc_address)| {
                    // This iterator contains the results after limit + offset are applied.
                    let ctid = self.ctid_value(doc_address);
                    SearchStateManager::set_result(
                        score.key.clone(),
                        score.bm25,
                        doc_address,
                        self.config.alias.clone(),
                    )
                    .expect("could not store search result in state manager");
                    (score.bm25, doc_address, score.key, ctid)
                })
                .collect()
        } else {
            let collector = TopDocs::with_limit(limit).and_offset(offset);
            self.searcher
                .search_with_executor(
                    self.query.as_ref(),
                    &collector,
                    executor,
                    tantivy::query::EnableScoring::Enabled {
                        searcher: &self.searcher,
                        statistics_provider: &self.searcher,
                    },
                )
                .expect("failed to search")
                .into_iter()
                .map(|(score, doc_address)| {
                    // This iterator contains the results after limit + offset are applied.
                    let (key, ctid) = self.key_and_ctid_value(doc_address);
                    SearchStateManager::set_result(
                        key.clone(),
                        score,
                        doc_address,
                        self.config.alias.clone(),
                    )
                    .expect("could not store search result in state manager");
                    (score, doc_address, key, ctid)
                })
                .collect()
        }
    }

    pub fn key_value(&self, doc_address: DocAddress) -> String {
        let retrieved_doc = self
            .searcher
            .doc(doc_address)
            .expect("could not retrieve document by address");

        let value = retrieved_doc
            .get_first(self.schema.key_field().id.0)
            .unwrap();

        match value {
            tantivy::schema::Value::Str(string) => string.clone(),
            tantivy::schema::Value::U64(u64) => format!("{:?}", u64),
            tantivy::schema::Value::I64(i64) => format!("{:?}", i64),
            tantivy::schema::Value::F64(f64) => format!("{:?}", f64),
            tantivy::schema::Value::Bool(bool) => format!("{:?}", bool),
            tantivy::schema::Value::Date(datetime) => datetime.into_primitive().to_string(),
            tantivy::schema::Value::Bytes(bytes) => String::from_utf8(bytes.clone()).unwrap(),
            _ => panic!("NO")
        }
    }

    pub fn ctid_value(&self, doc_address: DocAddress) -> u64 {
        let retrieved_doc = self
            .searcher
            .doc(doc_address)
            .expect("could not retrieve document by address");

        retrieved_doc
            .get_first(self.schema.ctid_field().id.0)
            .unwrap()
            .as_u64()
            .expect("could not access ctid field on document")
    }

    pub fn key_and_ctid_value(&self, doc_address: DocAddress) -> (String, u64) {
        let retrieved_doc = self
            .searcher
            .doc(doc_address)
            .expect("could not retrieve document by address");

        // let key = retrieved_doc
        //     .get_first(self.schema.key_field().id.0)
        //     .unwrap()
        //     .as_i64()
        //     .expect("could not access key field on document");
        let value = retrieved_doc
            .get_first(self.schema.key_field().id.0)
            .unwrap();

        let key = match value {
            tantivy::schema::Value::Str(string) => string.clone(),
            tantivy::schema::Value::U64(u64) => format!("{:?}", u64),
            tantivy::schema::Value::I64(i64) => format!("{:?}", i64),
            tantivy::schema::Value::F64(f64) => format!("{:?}", f64),
            tantivy::schema::Value::Bool(bool) => format!("{:?}", bool),
            tantivy::schema::Value::Date(datetime) => datetime.into_primitive().to_string(),
            tantivy::schema::Value::Bytes(bytes) => String::from_utf8(bytes.clone()).unwrap(),
            _ => panic!("NO")
        };

        let ctid = retrieved_doc
            .get_first(self.schema.ctid_field().id.0)
            .unwrap()
            .as_u64()
            .expect("could not access ctid field on document");
        (key, ctid)
    }

    /// A search method that deduplicates results based on key field. This is important for
    /// searches into the Tantivy index outside of Postgres index access methods. Postgres will
    /// filter out stale rows when using the index scan, but when scanning Tantivy directly,
    /// we risk returning deleted documents if a VACUUM hasn't been performed yet.
    pub fn search_dedup(
        &mut self,
        executor: &Executor,
    ) -> impl Iterator<Item = (Score, DocAddress)> {
        let search_results = self.search(executor);
        let mut dedup_map: HashMap<String, (Score, DocAddress)> = HashMap::new();
        let mut order_vec: Vec<String> = Vec::new();

        for (score, doc_addr, key, _) in search_results {
            let is_new_or_higher = match dedup_map.get(&key) {
                Some((_, existing_doc_addr)) => doc_addr > *existing_doc_addr,
                None => true,
            };
            if is_new_or_higher && dedup_map.insert(key.clone(), (score, doc_addr)).is_none() {
                // Key was not already present, remember the order of this key
                order_vec.push(key);
            }
        }

        order_vec
            .into_iter()
            .filter_map(move |key| dedup_map.remove(&key))
    }
}
