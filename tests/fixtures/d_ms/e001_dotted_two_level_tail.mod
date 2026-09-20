// inventory: e001_dotted_two_level_tail
// `alpha.foo.bar(1);` — a two-level head takes `prior` or `options`; 7.1 refuses `unexpected IDENTIFIER, expecting OPTIONS or PRIOR`.
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
alpha.foo.bar(1);
