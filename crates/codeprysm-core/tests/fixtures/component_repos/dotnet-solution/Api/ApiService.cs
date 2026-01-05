using MyApp.Shared;

namespace MyApp.Api
{
    public class ApiService
    {
        public string Handle() => $"api: {SharedLib.GetShared()}";
    }
}
