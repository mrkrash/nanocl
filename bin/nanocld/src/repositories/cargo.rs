use std::sync::Arc;

use diesel::prelude::*;

use nanocl_error::io::{IoError, IoResult};

use nanocl_stubs::{
  generic::{GenericFilter, GenericClause},
  cargo::Cargo,
  cargo_spec::{CargoSpecPartial, CargoSpec},
};

use crate::{
  gen_multiple, gen_where4string, utils,
  models::{Pool, CargoDb, SpecDb, CargoUpdateDb},
  schema::cargoes,
};

use super::generic::*;

impl RepositoryBase for CargoDb {}

impl RepositoryCreate for CargoDb {}

impl RepositoryUpdate for CargoDb {
  type UpdateItem = CargoUpdateDb;
}

impl RepositoryDelByPk for CargoDb {}

impl RepositoryReadBy for CargoDb {
  type Output = (CargoDb, SpecDb);

  fn get_pk() -> &'static str {
    "key"
  }

  fn gen_read_query(
    filter: &GenericFilter,
    is_multiple: bool,
  ) -> impl diesel::query_dsl::methods::LoadQuery<
    'static,
    diesel::PgConnection,
    Self::Output,
  >
  where
    Self::Output: Sized,
  {
    let r#where = filter.r#where.to_owned().unwrap_or_default();
    let mut query = cargoes::table
      .inner_join(crate::schema::specs::table)
      .into_boxed();
    if let Some(value) = r#where.get("key") {
      gen_where4string!(query, cargoes::key, value);
    }
    if let Some(value) = r#where.get("name") {
      gen_where4string!(query, cargoes::name, value);
    }
    if let Some(value) = r#where.get("namespace_name") {
      gen_where4string!(query, cargoes::namespace_name, value);
    }
    if is_multiple {
      gen_multiple!(query, cargoes::created_at, filter);
    }
    query
  }
}

impl RepositoryReadByTransform for CargoDb {
  type NewOutput = Cargo;

  fn transform(item: (CargoDb, SpecDb)) -> IoResult<Self::NewOutput> {
    let (cargodb, specdb) = item;
    let spec = specdb.try_to_cargo_spec()?;
    let item = cargodb.with_spec(&spec);
    Ok(item)
  }
}

impl WithSpec for CargoDb {
  type Output = Cargo;
  type Relation = CargoSpec;

  fn with_spec(self, r: &Self::Relation) -> Self::Output {
    Self::Output {
      namespace_name: self.namespace_name,
      created_at: self.created_at,
      spec: r.clone(),
    }
  }
}

impl CargoDb {
  /// Create a new cargo from its specification.
  pub(crate) async fn create_from_spec(
    nsp: &str,
    item: &CargoSpecPartial,
    version: &str,
    pool: &Pool,
  ) -> IoResult<Cargo> {
    let nsp = nsp.to_owned();
    let item = item.to_owned();
    let version = version.to_owned();
    // test if the name of the cargo include a . in the name and throw error if true
    if item.name.contains('.') {
      return Err(IoError::invalid_input(
        "CargoSpecPartial",
        "Name cannot contain a dot.",
      ));
    }
    let key = utils::key::gen_key(&nsp, &item.name);
    let new_spec = SpecDb::try_from_cargo_partial(&key, &version, &item)?;
    let spec = SpecDb::create_from(new_spec, pool)
      .await?
      .try_to_cargo_spec()?;
    let new_item = CargoDb {
      key,
      name: item.name,
      created_at: chrono::Utc::now().naive_utc(),
      namespace_name: nsp,
      spec_key: spec.key,
    };
    let item = CargoDb::create_from(new_item, pool).await?.with_spec(&spec);
    Ok(item)
  }

  /// Update a cargo from its specification.
  pub(crate) async fn update_from_spec(
    key: &str,
    item: &CargoSpecPartial,
    version: &str,
    pool: &Pool,
  ) -> IoResult<Cargo> {
    let version = version.to_owned();
    let mut cargo = CargoDb::transform_read_by_pk(key, pool).await?;
    let new_spec = SpecDb::try_from_cargo_partial(key, &version, item)?;
    let spec = SpecDb::create_from(new_spec, pool)
      .await?
      .try_to_cargo_spec()?;
    let new_item = CargoUpdateDb {
      name: Some(item.name.to_owned()),
      spec_key: Some(spec.key),
      ..Default::default()
    };
    CargoDb::update_pk(key, new_item, pool).await?;
    cargo.spec = spec;
    Ok(cargo)
  }

  /// Find cargoes by namespace.
  pub(crate) async fn read_by_namespace(
    name: &str,
    pool: &Pool,
  ) -> IoResult<Vec<Cargo>> {
    let filter = GenericFilter::new()
      .r#where("namespace_name", GenericClause::Eq(name.to_owned()));
    CargoDb::transform_read_by(&filter, pool).await
  }

  /// Count cargoes by namespace.
  pub(crate) async fn count_by_namespace(
    nsp: &str,
    pool: &Pool,
  ) -> IoResult<i64> {
    let nsp = nsp.to_owned();
    let pool = Arc::clone(pool);
    let count = ntex::web::block(move || {
      let mut conn = utils::store::get_pool_conn(&pool)?;
      let count = cargoes::table
        .filter(cargoes::namespace_name.eq(nsp))
        .count()
        .get_result(&mut conn)
        .map_err(Self::map_err)?;
      Ok::<_, IoError>(count)
    })
    .await?;
    Ok(count)
  }
}
