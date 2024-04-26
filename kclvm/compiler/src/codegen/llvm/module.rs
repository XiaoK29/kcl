// Copyright The KCL Authors. All rights reserved.

use kclvm_ast::ast;
use kclvm_ast::walker::TypedResultWalker;

use super::context::LLVMCodeGenContext;
use crate::codegen::error as kcl_error;
use crate::codegen::traits::ValueMethods;
use std::str;

impl<'ctx> LLVMCodeGenContext<'ctx> {
    pub fn compile_module_import_and_types(&self, module: &'ctx ast::Module) {
        for stmt in &module.body {
            match &stmt.node {
                ast::Stmt::Import(import_stmt) => {
                    self.walk_import_stmt(import_stmt)
                        .expect(kcl_error::COMPILE_ERROR_MSG);
                }
                ast::Stmt::Schema(schema_stmt) => {
                    // Pre define global types with undefined values
                    self.predefine_global_types(&schema_stmt.name.node);
                    self.walk_schema_stmt(schema_stmt)
                        .expect(kcl_error::COMPILE_ERROR_MSG);
                }
                ast::Stmt::Rule(rule_stmt) => {
                    // Pre define global types with undefined values
                    self.predefine_global_types(&rule_stmt.name.node);
                    self.walk_rule_stmt(rule_stmt)
                        .expect(kcl_error::COMPILE_ERROR_MSG);
                }
                _ => {}
            };
        }
    }

    pub fn predefine_global_types(&self, name: &str) {
        // Store or add the variable in the scope
        let function = self.undefined_value();
        if !self.store_variable(name, function) {
            let global_var_ptr = self.new_global_kcl_value_ptr("");
            self.builder.build_store(global_var_ptr, function);
            self.add_variable(name, global_var_ptr);
        }
    }

    /// Predefine all global variables.
    #[inline]
    pub(crate) fn predefine_global_vars(&self, module: &'ctx ast::Module) {
        self.emit_global_vars(&module.body);
    }

    fn emit_global_vars(&self, body: &'ctx [Box<ast::Node<ast::Stmt>>]) {
        for stmt in body {
            match &stmt.node {
                ast::Stmt::Unification(unification_stmt) => {
                    let names = &unification_stmt.target.node.names;
                    if names.len() == 1 {
                        self.add_or_update_global_variable(&names[0].node, self.undefined_value());
                    }
                }
                ast::Stmt::Assign(assign_stmt) => {
                    for target in &assign_stmt.targets {
                        let names = &target.node.names;
                        if names.len() == 1 {
                            self.add_or_update_global_variable(
                                &names[0].node,
                                self.undefined_value(),
                            );
                        }
                    }
                }
                ast::Stmt::If(if_stmt) => {
                    self.emit_global_vars(&if_stmt.body);
                    self.emit_global_vars(&if_stmt.orelse);
                }
                _ => {}
            }
        }
    }

    /// Compile AST Modules, which requires traversing three times.
    /// 1. scan all possible global variables and allocate undefined values to global pointers.
    /// 2. build all user-defined schema/rule types.
    /// 3. generate all LLVM IR codes for the third time.
    pub(crate) fn compile_ast_modules(&self, modules: &'ctx [ast::Module]) {
        // Scan global variables
        for ast_module in modules {
            {
                self.filename_stack
                    .borrow_mut()
                    .push(ast_module.filename.clone());
            }
            // Pre define global variables with undefined values
            self.predefine_global_vars(ast_module);
            {
                self.filename_stack.borrow_mut().pop();
            }
        }
        // Scan global types
        for ast_module in modules {
            {
                self.filename_stack
                    .borrow_mut()
                    .push(ast_module.filename.clone());
            }
            self.compile_module_import_and_types(ast_module);
            {
                self.filename_stack.borrow_mut().pop();
            }
        }
        // Compile the ast module in the pkgpath.
        for ast_module in modules {
            {
                self.filename_stack
                    .borrow_mut()
                    .push(ast_module.filename.clone());
            }
            self.walk_module(ast_module)
                .expect(kcl_error::COMPILE_ERROR_MSG);
            {
                self.filename_stack.borrow_mut().pop();
            }
        }
    }
}
