//! Runtime object system: values, environments, the virtual machine, and
//! promises.

mod environment;
pub(crate) mod http_stream;
mod promise;
mod value;
mod vm;

pub use crate::async_runtime::{
    AsyncCompletion, AsyncCompletionData, AsyncCompletionId, AsyncCompletionResult,
    AsyncCompletionSender, AsyncHttpResponse,
};
pub use environment::{Binding, Environment};
pub use promise::{Promise, PromiseState};
pub use value::{
    bool_obj, format_number, new_error, new_error_object, new_named_error, num_obj, str_obj,
    strict_equal, ArrayData, Builtin, BuiltinFn, CallContext, Class, ErrorData, Function, HashData,
    Instance, MapData, NativeCtor, Object, RegexpData, SetData,
};
pub use vm::{
    vm_error, EnvRef, EvaluatorFn, ImporterFn, NodeRef, VirtualMachine, VmOutput,
    EXEC_MODE_BYTECODE, EXEC_MODE_TREEWALK,
};
