// inventory: tuple loop binds both names in each iteration
var y_1 y_3;
model;
@#for (i,j) in [(1,2),(3,4)]
y_@{i} = @{j};
@#endfor
end;
