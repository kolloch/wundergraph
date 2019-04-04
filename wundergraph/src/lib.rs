#![deny(missing_debug_implementations, missing_copy_implementations)]
#![warn(
    clippy::option_unwrap_used,
    clippy::result_unwrap_used,
    clippy::print_stdout,
    clippy::wrong_pub_self_convention,
    clippy::mut_mut,
    clippy::non_ascii_literal,
    clippy::similar_names,
    clippy::unicode_not_nfc,
    clippy::enum_glob_use,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::used_underscore_binding,
    clippy::cargo_common_metadata,
    clippy::dbg_macro,
    clippy::doc_markdown,
    clippy::filter_map,
    clippy::map_flatten,
    clippy::match_same_arms,
    clippy::needless_borrow,
    clippy::needless_pass_by_value,
    clippy::option_map_unwrap_or,
    clippy::option_map_unwrap_or_else,
    clippy::redundant_clone,
    clippy::result_map_unwrap_or_else,
    clippy::unnecessary_unwrap,
    clippy::unseparated_literal_suffix,
    clippy::wildcard_dependencies

)]
#![allow(clippy::type_complexity)]

#[doc(hidden)]
#[macro_use]
pub extern crate diesel;
#[macro_use]
#[doc(hidden)]
pub extern crate juniper;
#[doc(hidden)]
pub extern crate indexmap;
#[macro_use]
pub extern crate failure;
#[doc(hidden)]
#[macro_use]
pub extern crate log;
pub extern crate paste;

#[allow(unused_imports)]
#[macro_use]
extern crate wundergraph_derive;
#[doc(hidden)]
pub use wundergraph_derive::*;

#[doc(hidden)]
pub mod diesel_ext;
pub mod error;
pub mod filter;
pub mod helper;
pub mod mutations;
pub mod order;
pub mod query_helper;
//pub mod query_modifier;
pub mod scalar;
#[macro_use]
mod macros;
pub mod graphql_type;

use self::error::WundergraphError;
use self::helper::FromLookAheadValue;
use self::scalar::WundergraphScalarValue;

use crate::helper::primary_keys::{PrimaryKeyArgument, UnRef};
use crate::query_helper::placeholder::*;
use diesel::associations::HasTable;
use diesel::backend::Backend;
use diesel::dsl::SqlTypeOf;
use diesel::expression::NonAggregate;
use diesel::query_builder::{BoxedSelectStatement, QueryFragment};
use diesel::query_dsl::methods::BoxedDsl;
use diesel::query_dsl::methods::{LimitDsl, OffsetDsl, SelectDsl};
use diesel::r2d2;
use diesel::EqAll;
use diesel::Identifiable;
use diesel::QuerySource;
use diesel::{AppearsOnTable, Connection, QueryDsl, Table};
use failure::Error;
use juniper::{Executor, LookAheadSelection, Selection};

pub trait WundergraphContext {
    type Connection: Connection + 'static;
    fn get_connection(&self) -> &Self::Connection;
}

impl<Conn> WundergraphContext for r2d2::PooledConnection<r2d2::ConnectionManager<Conn>>
where
    Conn: Connection + 'static,
    Self: Connection<Backend = Conn::Backend>,
{
    type Connection = Self;

    fn get_connection(&self) -> &Self {
        self
    }
}

pub type BoxedQuery<'a, L, DB, Ctx> = BoxedSelectStatement<
    'a,
    SqlTypeOfPlaceholder<
        <L as LoadingHandler<DB, Ctx>>::FieldList,
        DB,
        <L as LoadingHandler<DB, Ctx>>::PrimaryKeyIndex,
        <L as HasTable>::Table,
        Ctx,
    >,
    <L as HasTable>::Table,
    DB,
>;

pub trait QueryModifier<L, DB>: WundergraphContext + Sized
where
    L: LoadingHandler<DB, Self>,
    DB: Backend + ApplyOffset + 'static,
{
    fn modify_query<'a>(
        &self,
        select: &LookAheadSelection<'_, WundergraphScalarValue>,
        query: BoxedQuery<'a, L, DB, Self>,
    ) -> Result<BoxedQuery<'a, L, DB, Self>, Error>;
}

impl<Conn, DB, T> QueryModifier<T, DB> for r2d2::PooledConnection<r2d2::ConnectionManager<Conn>>
where
    T: LoadingHandler<DB, Self>,
    Conn: Connection<Backend = DB> + 'static,
    Self: Connection<Backend = DB> + 'static,
    DB: Backend + ApplyOffset + 'static,
    T::Table: 'static,
    <T::Table as QuerySource>::FromClause: QueryFragment<DB>,
    DB::QueryBuilder: Default,
{
    fn modify_query<'a>(
        &self,
        _select: &LookAheadSelection<'_, WundergraphScalarValue>,
        query: BoxedSelectStatement<
            'a,
            SqlTypeOfPlaceholder<T::FieldList, DB, T::PrimaryKeyIndex, T::Table, Self>,
            T::Table,
            DB,
        >,
    ) -> Result<
        BoxedSelectStatement<
            'a,
            SqlTypeOfPlaceholder<T::FieldList, DB, T::PrimaryKeyIndex, T::Table, Self>,
            T::Table,
            DB,
        >,
        Error,
    > {
        Ok(query)
    }
}

use crate::diesel_ext::BoxableFilter;
use crate::filter::build_filter::BuildFilter;
use crate::filter::inner_filter::InnerFilter;
use crate::filter::Filter;
use crate::query_helper::order::BuildOrder;
use crate::query_helper::select::BuildSelect;
use crate::query_helper::tuple::IsPrimaryKeyIndex;
use diesel::query_dsl::methods::FilterDsl;
use diesel::sql_types::{Bool, HasSqlType};
use diesel::BoxableExpression;
use juniper::LookAheadValue;

pub trait LoadingHandler<DB, Ctx>: HasTable + Sized
where
    DB: Backend + ApplyOffset + 'static,
{
    type Columns: BuildOrder<Self::Table, DB>
        + BuildSelect<
            Self::Table,
            DB,
            SqlTypeOfPlaceholder<Self::FieldList, DB, Self::PrimaryKeyIndex, Self::Table, Ctx>,
        >;
    type FieldList: WundergraphFieldList<DB, Self::PrimaryKeyIndex, Self::Table, Ctx>;

    type PrimaryKeyIndex: Default + IsPrimaryKeyIndex;
    type Filter: InnerFilter + BuildFilter<DB> + 'static;

    const FIELD_NAMES: &'static [&'static str];
    const TYPE_NAME: &'static str;

    fn load<'a>(
        select: &LookAheadSelection<'_, WundergraphScalarValue>,
        selection: Option<&'_ [Selection<'_, WundergraphScalarValue>]>,
        executor: &Executor<'_, Ctx, WundergraphScalarValue>,
        query: BoxedQuery<'a, Self, DB, Ctx>,
    ) -> Result<Vec<juniper::Value<WundergraphScalarValue>>, Error>
    where
        DB: HasSqlType<
            SqlTypeOfPlaceholder<Self::FieldList, DB, Self::PrimaryKeyIndex, Self::Table, Ctx>,
        >,
        Ctx: WundergraphContext + QueryModifier<Self, DB>,
        Ctx::Connection: Connection<Backend = DB>,
        DB::QueryBuilder: Default,
        <Self::Table as QuerySource>::FromClause: QueryFragment<DB>,
    {
        use diesel::RunQueryDsl;
        let ctx = executor.context();
        let conn = ctx.get_connection();
        let query = ctx.modify_query(select, query)?;
        if cfg!(feature = "debug") {
            #[allow(clippy::use_debug, clippy::print_stdout)]
            {
                println!("{:?}", diesel::debug_query(&query));
            }
        }
        let placeholder = <_ as RunQueryDsl<_>>::load(query, conn)?;
        Ok(Self::FieldList::resolve(
            placeholder,
            select,
            selection,
            Self::FIELD_NAMES,
            executor,
        )?)
    }

    fn load_by_primary_key<'a>(
        select: &LookAheadSelection<'_, WundergraphScalarValue>,
        selection: Option<&'_ [Selection<'_, WundergraphScalarValue>]>,
        executor: &Executor<'_, Ctx, WundergraphScalarValue>,
        mut query: BoxedQuery<'a, Self, DB, Ctx>,
    ) -> Result<Option<juniper::Value<WundergraphScalarValue>>, Error>
    where
        Self: 'static,
        &'static Self: Identifiable,
        Ctx: WundergraphContext + QueryModifier<Self, DB>,
        Ctx::Connection: Connection<Backend = DB>,
        <&'static Self as Identifiable>::Id: UnRef<'static>,
        <Self::Table as Table>::PrimaryKey:
            EqAll<<<&'static Self as Identifiable>::Id as UnRef<'static>>::UnRefed>,
        <<Self::Table as Table>::PrimaryKey as EqAll<
            <<&'static Self as Identifiable>::Id as UnRef<'static>>::UnRefed,
        >>::Output: AppearsOnTable<Self::Table> + NonAggregate + QueryFragment<DB>,
        PrimaryKeyArgument<'static, Self::Table, (), <&'static Self as Identifiable>::Id>:
            FromLookAheadValue,
        DB: HasSqlType<
            SqlTypeOfPlaceholder<Self::FieldList, DB, Self::PrimaryKeyIndex, Self::Table, Ctx>,
        >,
        DB::QueryBuilder: Default,
        <Self::Table as QuerySource>::FromClause: QueryFragment<DB>,
    {
        use juniper::LookAheadMethods;
        let v = select
            .argument("primaryKey")
            .ok_or(WundergraphError::NoPrimaryKeyArgumentFound)?;
        let key = PrimaryKeyArgument::<
            Self::Table,
            _,
            <&'static Self as Identifiable>::Id,
            >::from_look_ahead(v.value())
            .ok_or(WundergraphError::NoPrimaryKeyArgumentFound)?;
        query = <_ as QueryDsl>::filter(query, Self::table().primary_key().eq_all(key.values));
        query = <_ as QueryDsl>::limit(query, 1);
        let res = Self::load(select, selection, executor, query)?;
        Ok(res.into_iter().next())
    }

    fn build_query<'a>(
        select: &LookAheadSelection<'_, WundergraphScalarValue>,
    ) -> Result<BoxedQuery<'a, Self, DB, Ctx>, Error>
    where
        Self::Table: BoxedDsl<
                'a,
                DB,
                Output = BoxedSelectStatement<
                    'a,
                    SqlTypeOf<<Self::Table as Table>::AllColumns>,
                    Self::Table,
                    DB,
                >,
            > + 'static,
        <Self::Filter as BuildFilter<DB>>::Ret: AppearsOnTable<Self::Table>,
    {
        let mut query =
            <_ as SelectDsl<_>>::select(Self::table().into_boxed(), Self::get_select(select)?);

        query = Self::apply_filter(query, select)?;
        query = Self::apply_limit(query, select)?;
        query = Self::apply_offset(query, select)?;
        query = Self::apply_order(query, select)?;

        Ok(query)
    }

    fn get_select(
        select: &LookAheadSelection<'_, WundergraphScalarValue>,
    ) -> Result<
        Box<
            dyn BoxableExpression<
                Self::Table,
                DB,
                SqlType = SqlTypeOfPlaceholder<
                    Self::FieldList,
                    DB,
                    Self::PrimaryKeyIndex,
                    Self::Table,
                    Ctx,
                >,
            >,
        >,
        Error,
    > {
        use juniper::LookAheadMethods;
        <Self::Columns as BuildSelect<Self::Table, DB, _>>::build_select(
            select,
            |local_index| {
                Self::FieldList::map_table_field(local_index, |global| Self::FIELD_NAMES[global])
                    .expect("Field is there")
            },
            Self::PrimaryKeyIndex::is_index,
            (0..Self::FieldList::NON_TABLE_FIELD_COUNT).any(|i| {
                Self::FieldList::map_non_table_field(i, |global| {
                    select.has_child(Self::FIELD_NAMES[global])
                })
                .unwrap_or(false)
            }),
        )
    }

    fn get_filter(
        input: &LookAheadValue<'_, WundergraphScalarValue>,
    ) -> Result<Option<Box<dyn BoxableFilter<Self::Table, DB, SqlType = Bool>>>, Error>
    where
        Self::Table: 'static,
        <Self::Filter as BuildFilter<DB>>::Ret: AppearsOnTable<Self::Table>,
    {
        Ok(
            <Filter<Self::Filter, Self::Table> as FromLookAheadValue>::from_look_ahead(input)
                .and_then(<_ as BuildFilter<DB>>::into_filter),
        )
    }

    fn apply_filter<'a>(
        query: BoxedQuery<'a, Self, DB, Ctx>,
        select: &LookAheadSelection<'_, WundergraphScalarValue>,
    ) -> Result<BoxedQuery<'a, Self, DB, Ctx>, Error>
    where
        Self::Table: 'static,
        <Self::Filter as BuildFilter<DB>>::Ret: AppearsOnTable<Self::Table>,
    {
        use juniper::LookAheadMethods;
        if let Some(filter) = select.argument("filter") {
            if let Some(filter) = Self::get_filter(filter.value())? {
                Ok(<_ as FilterDsl<_>>::filter(query, filter))
            } else {
                Ok(query)
            }
        } else {
            Ok(query)
        }
    }

    fn apply_order<'a>(
        mut query: BoxedQuery<'a, Self, DB, Ctx>,
        select: &LookAheadSelection<'_, WundergraphScalarValue>,
    ) -> Result<BoxedQuery<'a, Self, DB, Ctx>, Error>
    where
        Self::Table: 'static,
    {
        use juniper::{LookAheadArgument, LookAheadMethods, LookAheadValue};
        if let Some(LookAheadValue::List(order)) =
            select.argument("order").map(LookAheadArgument::value)
        {
            let order_stmts = <Self::Columns as BuildOrder<Self::Table, DB>>::build_order(
                order,
                |local_index| {
                    Self::FieldList::map_table_field(local_index, |global| {
                        Self::FIELD_NAMES[global]
                    })
                    .expect("Field is there")
                },
            )?;
            for s in order_stmts {
                query = query.then_order_by(s);
            }
            Ok(query)
        } else {
            Ok(query)
        }
    }

    fn apply_limit<'a>(
        query: BoxedQuery<'a, Self, DB, Ctx>,
        select: &LookAheadSelection<'_, WundergraphScalarValue>,
    ) -> Result<BoxedQuery<'a, Self, DB, Ctx>, Error> {
        use juniper::LookAheadMethods;
        if let Some(limit) = select.argument("limit") {
            Ok(<_ as LimitDsl>::limit(
                query,
                i64::from_look_ahead(limit.value())
                    .ok_or(WundergraphError::CouldNotBuildFilterArgument)?,
            ))
        } else {
            Ok(query)
        }
    }

    fn apply_offset<'a>(
        query: BoxedQuery<'a, Self, DB, Ctx>,
        select: &LookAheadSelection<'_, WundergraphScalarValue>,
    ) -> Result<BoxedQuery<'a, Self, DB, Ctx>, Error> {
        <DB as ApplyOffset>::apply_offset::<Self, Ctx>(query, select)
    }

    fn field_description(_idx: usize) -> Option<&'static str> {
        None
    }

    fn type_description() -> Option<&'static str> {
        None
    }

    #[allow(clippy::option_option)]
    fn field_deprecation(_idx: usize) -> Option<Option<&'static str>> {
        None
    }
}

pub trait ApplyOffset: Backend {
    fn apply_offset<'a, L, Ctx>(
        query: BoxedQuery<'a, L, Self, Ctx>,
        select: &LookAheadSelection<'_, WundergraphScalarValue>,
    ) -> Result<BoxedQuery<'a, L, Self, Ctx>, Error>
    where
        L: LoadingHandler<Self, Ctx>;
}

#[cfg(feature = "postgres")]
impl ApplyOffset for diesel::pg::Pg {
    fn apply_offset<'a, L, Ctx>(
        query: BoxedQuery<'a, L, Self, Ctx>,
        select: &LookAheadSelection<'_, WundergraphScalarValue>,
    ) -> Result<BoxedQuery<'a, L, Self, Ctx>, Error>
    where
        L: LoadingHandler<Self, Ctx>,
    {
        use juniper::LookAheadMethods;
        if let Some(offset) = select.argument("offset") {
            Ok(<_ as OffsetDsl>::offset(
                query,
                i64::from_look_ahead(offset.value())
                    .ok_or(WundergraphError::CouldNotBuildFilterArgument)?,
            ))
        } else {
            Ok(query)
        }
    }
}

#[cfg(feature = "sqlite")]
impl ApplyOffset for diesel::sqlite::Sqlite {
    fn apply_offset<'a, L, Ctx>(
        query: BoxedQuery<'a, L, Self, Ctx>,
        select: &LookAheadSelection<'_, WundergraphScalarValue>,
    ) -> Result<BoxedQuery<'a, L, Self, Ctx>, Error>
    where
        L: LoadingHandler<Self, Ctx>,
    {
        use juniper::LookAheadMethods;
        if let Some(offset) = select.argument("offset") {
            let q = <_ as OffsetDsl>::offset(
                query,
                i64::from_look_ahead(offset.value())
                    .ok_or(WundergraphError::CouldNotBuildFilterArgument)?,
            );
            if select.argument("limit").is_some() {
                Ok(q)
            } else {
                Ok(<_ as LimitDsl>::limit(q, -1))
            }
        } else {
            Ok(query)
        }
    }
}

#[macro_export]
#[doc(hidden)]
/// Used by `wundergraph_derives`, which can't access `$crate`
macro_rules! __use_everything {
    () => {
        pub use $crate::*;
    };
}
