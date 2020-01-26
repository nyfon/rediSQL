use parser::common::CommandV2;
use parser::exec::Exec;
use parser::exec::ToExecute;

use redisql_lib::redis as r;
use redisql_lib::redis::do_execute;
use redisql_lib::redis::do_query;
use redisql_lib::redis::LoopData;
use redisql_lib::redis::RedisReply;
use redisql_lib::redis::Returner;
use redisql_lib::redis::StatementCache;
use redisql_lib::redis_type::BlockedClient;
use redisql_lib::redis_type::ReplicateVerbatim;

use crate::common::{free_privdata, reply, timeout};

#[allow(non_snake_case)]
pub extern "C" fn Exec_v2(
    ctx: *mut r::rm::ffi::RedisModuleCtx,
    argv: *mut *mut r::rm::ffi::RedisModuleString,
    argc: ::std::os::raw::c_int,
) -> i32 {
    let context = r::rm::Context::new(ctx);
    let argvector = match r::create_argument(argv, argc) {
        Ok(argvector) => argvector,
        Err(mut error) => {
            return error.reply(&context);
        }
    };
    let command: Exec = match CommandV2::parse(argvector) {
        Ok(comm) => comm,
        Err(mut e) => return e.reply(&context),
    };
    let t = std::time::Instant::now()
        + std::time::Duration::from_secs(10);
    let key = command.key(&context);
    if !command.is_now() {
        match key.get_channel() {
            Err(mut e) => e.reply(&context),
            Ok(ch) => {
                let blocked_client = BlockedClient::new(
                    &context,
                    reply,
                    timeout,
                    free_privdata,
                    10_000,
                );
                let command = command.get_command(t, blocked_client);
                ReplicateVerbatim(&context);
                match ch.send(command) {
                    Err(e) => {
                        dbg!(
                            "Error in sending the command!",
                            e.to_string()
                        );
                        r::rm::ffi::REDISMODULE_OK
                    }
                    _ => r::rm::ffi::REDISMODULE_OK,
                }
            }
        }
    } else {
        let db = match key.get_db() {
            Ok(k) => k,
            Err(mut e) => return e.reply(&context),
        };
        let read_only = command.is_read_only();
        let return_method = command.get_return_method();
        match command.get_to_execute() {
            ToExecute::Query(s) => {
                let mut res = match read_only {
                    true => match do_query(&db, s) {
                        Ok(r) => r.create_data_to_return(
                            &context,
                            &return_method,
                            t,
                        ),
                        Err(e) => e.create_data_to_return(
                            &context,
                            &return_method,
                            t,
                        ),
                    },
                    false => match do_execute(&db, s) {
                        Ok(r) => {
                            ReplicateVerbatim(&context);
                            r.create_data_to_return(
                                &context,
                                &return_method,
                                t,
                            )
                        }
                        Err(e) => e.create_data_to_return(
                            &context,
                            &return_method,
                            t,
                        ),
                    },
                };
                res.reply(&context)
            }
            ToExecute::Statement(id) => {
                let loop_data = match key.get_loop_data() {
                    Ok(k) => k,
                    Err(mut e) => return e.reply(&context),
                };

                let mut result = match read_only {
                    true => {
                        match loop_data
                            .get_replication_book()
                            .query_statement(&id, &command.args)
                        {
                            Ok(r) => r.create_data_to_return(
                                &context,
                                &return_method,
                                t,
                            ),
                            Err(e) => e.create_data_to_return(
                                &context,
                                &return_method,
                                t,
                            ),
                        }
                    }
                    false => {
                        match loop_data
                            .get_replication_book()
                            .exec_statement(&id, &command.args)
                        {
                            Ok(r) => r.create_data_to_return(
                                &context,
                                &return_method,
                                t,
                            ),
                            Err(e) => e.create_data_to_return(
                                &context,
                                &return_method,
                                t,
                            ),
                        }
                    }
                };
                result.reply(&context)
            }
        }
    }
}
