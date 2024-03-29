#[cfg(test)]
pub mod test_util {
    #[macro_export]
    macro_rules! current_source_dir {
        () => {{
            const FILE: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/", file!());
            Path::new(FILE).parent().unwrap()
        }};
    }

    #[macro_export]
    macro_rules! assert_error_result {
        ($expected:expr,$result:expr) => {
            if let Err(err) = $result {
                fn assert_err_eq<E: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static>(
                    expected: E,
                    err: anyhow::Error,
                ) {
                    if let Ok(err) = err.downcast::<E>() {
                        pretty_assertions::assert_eq!(format!("{}", expected), format!("{}", err));
                    } else {
                        panic!("unexpected error type");
                    }
                }
                assert_err_eq($expected, err);
            } else {
                panic!("unexpected result ok");
            }
        };
    }
}
#[macro_export]
macro_rules! join_futures {
    ($futures:expr) => {{
        let futures = $futures.map(|f| tokio::spawn(f)).collect::<Vec<_>>();
        async {
            let mut targets = vec![];
            let mut errs = vec![];
            for future in futures.into_iter() {
                let result = future.await.unwrap();
                match result {
                    Ok(target) => targets.push(target),
                    Err(err) => errs.push(err),
                }
            }
            if errs.is_empty() {
                Ok(targets)
            } else {
                Err(errs)
            }
        }
    }};
}

#[macro_export]
macro_rules! flex_eprintln{
    ($quiet:expr,$($arg:tt)*) =>{{
        if !$quiet {
            eprintln!($($arg)*);
        }
    }};
}
