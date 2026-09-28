// inventory: a valid macro builtin beyond this bounded expander stays visible
@#define n = length([1,2])
var y;
model;
y = @{n};
end;
