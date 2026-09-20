// inventory: d_ms_e001_native_assign_pair
// The over-refusal this slice fixes: an undeclared head with no `;` is native
// MATLAB text to 7.1's lexer, which accepts the file. No E001 may fire.
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
aaaa = 1
bbbb = 2;
