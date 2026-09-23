// inventory: e001_opener_var_statement_scoped
// The control: `varobs` carries a `DYNARE_STATEMENT`-scoped lexer rule too, so it
// ends the declaration list and 7.1 refuses the file with `syntax error, unexpected
// VAROBS`.
var y varobs;
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e;
end;
