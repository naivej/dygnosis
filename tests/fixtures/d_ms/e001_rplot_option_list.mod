// inventory: e001_rplot_option_list
// `rplot(periods=10);` — `rplot` takes a symbol list; 7.1 refuses `unexpected '('`.
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
rplot(periods=10);
