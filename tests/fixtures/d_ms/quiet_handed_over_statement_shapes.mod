// inventory: quiet_handed_over_statement_shapes
// The legal shapes of the handed-over statements: `dsample 10;`, `dsample 10 20;`, `rplot y, c;`,
// `smoother2histval;`, `smoother2histval(period=1);`, `var_remove alpha;`, `database myfile;` and
// `alpha.options(init=1);`. 7.1 accepts every one, so the sweep must leave them all alone.
// `var_remove alpha;` comes last: it drops `alpha`, so any later line naming that parameter would
// be a different refusal of 7.1's own.
var y c k;
varexo e;
parameters alpha beta gamma;
alpha = 0.36;
beta = 0.99;
gamma = 0.5;
model;
c = alpha*y + beta*c(-1) + e;
y = beta*y(-1) + c;
k = y;
end;
initval;
y = 0;
c = 0;
k = 0;
end;
shocks;
var e; stderr 0.1;
end;
dsample 10;
dsample 10 20;
rplot y, c;
smoother2histval;
smoother2histval(period=1);
database myfile;
alpha.options(init=1);
var_remove alpha;
