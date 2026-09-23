// inventory: e001_opener_var_decl_semi_missing
// The control: a genuinely missing `;` before the next declaration. 7.1 refuses with
// `syntax error, unexpected VAREXO`, and this must stay **E001**.
var y
varexo e;
parameters rho;
rho = 0.95;

model;
y = rho * y(-1) + e;
end;

shocks;
var e; stderr 0.01;
end;

stoch_simul(order = 1, nograph);
