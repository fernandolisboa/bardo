---
id: connect-youtube
title: Conectar o YouTube
group: publishing
---

# Conectar o YouTube

O Bardo entra no YouTube com um cliente OAuth que você registra no seu próprio projeto do Google Cloud. O Bardo não traz cliente próprio, então a cota, a tela de consentimento e qualquer auditoria são do seu projeto. Este guia configura esse projeto uma vez; depois disso, cada canal conecta pelo cartão da conta de rede em poucos cliques.

<a id="need"></a>
## Do que você precisa

- Uma conta do Google dona do canal do YouTube (ou que o gerencia).
- Acesso ao [console do Google Cloud](https://console.cloud.google.com/).

<a id="project"></a>
## 1. Crie um projeto e ative as APIs

1. No console do Google Cloud, [crie um projeto](https://console.cloud.google.com/projectcreate) (por exemplo `bardo`), ou escolha o que você já usa para a chave da YouTube Data API.
2. Com esse projeto selecionado, abra a página de cada API em **APIs e serviços › Biblioteca** e escolha **Ativar**:
   - [YouTube Data API v3](https://console.cloud.google.com/apis/library/youtube.googleapis.com) (envios, agendamento, o canal conectado);
   - [YouTube Analytics API](https://console.cloud.google.com/apis/library/youtubeanalytics.googleapis.com) (relatórios de visualizações, tempo de exibição e receita).

Sem elas, conectar falha com "O acesso foi recusado. Ative a YouTube Data API v3 e a YouTube Analytics API…".

<a id="consent"></a>
## 2. Configure a tela de consentimento

Abra a [Google Auth Platform](https://console.cloud.google.com/auth/overview). Um projeto que ainda não tem nada mostra **Primeiros passos** (*Get started*), que pede de uma vez o nome do app, o seu e-mail de suporte, o público-alvo (**Externo**) e um e-mail de contato; depois confira cada página abaixo.

1. [Branding](https://console.cloud.google.com/auth/branding): dê um nome ao app (por exemplo `Bardo`) e o seu e-mail de suporte.
2. [Público-alvo](https://console.cloud.google.com/auth/audience): escolha **Externo**. Enquanto o app estiver em **Teste**, adicione a sua própria conta do Google em **Usuários de teste**.
3. [Acesso a dados](https://console.cloud.google.com/auth/scopes): escolha **Adicionar ou remover escopos** e adicione estes (cole em **Adicionar escopos manualmente**) (o Bardo pede os quatro de uma vez, porque um app para computador não consegue adicionar escopos depois):
   - `https://www.googleapis.com/auth/youtube.upload`
   - `https://www.googleapis.com/auth/youtube`
   - `https://www.googleapis.com/auth/yt-analytics.readonly`
   - `https://www.googleapis.com/auth/yt-analytics-monetary.readonly`
4. De volta em [Público-alvo](https://console.cloud.google.com/auth/audience), escolha **Publicar app** para passá-lo para **Em produção**.

Por que produção: o Google encerra toda autorização dada a um app em **Teste** sete dias depois do consentimento, token de renovação incluído. O Bardo mostraria **Reconexão necessária** toda semana. Um app **Em produção** usado só por você não precisa da verificação do Google; o Google mostra uma tela de "app não verificado" no consentimento, em que você continua por **Avançado › Acessar (nome do app)**. Apps não verificados têm limite de 100 usuários no total, que um uso pessoal nunca atinge.

Os nomes dos menus podem aparecer em inglês, conforme o idioma do seu console: *APIs & Services › Library*, *Audience*, *Data Access*, *Publish app*, *In production*.

<a id="client"></a>
## 3. Crie o cliente OAuth

1. Abra [Google Auth Platform › Clientes](https://console.cloud.google.com/auth/clients/create) para criar um cliente.
2. Tipo de aplicativo: **App para computador**. Dê um nome (por exemplo `Bardo desktop`).
3. Escolha **Criar** e copie na hora o **ID do cliente** e a **Chave secreta do cliente**: o Google mostra a chave secreta inteira só quando o cliente é criado e, depois, apenas os quatro últimos caracteres. O ID termina em `.apps.googleusercontent.com`; a chave secreta normalmente começa com `GOCSPX-`. Se você perder a chave secreta, adicione uma nova ao cliente e salve essa no Bardo.

Um cliente para computador não precisa de endereço de redirecionamento: o Bardo escuta uma vez em `127.0.0.1`, numa porta aleatória, enquanto você dá o consentimento, e o Google aceita qualquer porta de loopback em clientes para computador.

<a id="save"></a>
## 4. Salve o cliente no Bardo

1. Abra [Configurações › Redes](bardo:go/settings/networks).
2. Cole o ID e a chave secreta do cliente em **YouTube · cliente OAuth do Google** e escolha **Salvar**.

Os dois ficam no Gerenciador de Credenciais do Windows, na sua conta do Windows, por perfil do Bardo. Eles nunca chegam ao banco de dados, aos logs nem às mensagens de erro do Bardo, e a chave secreta não aparece mais na tela; o cartão mostra os quatro últimos caracteres dela.

<a id="connect"></a>
## 5. Conecte o canal

1. Abra [Contas](bardo:go/accounts), escolha o canal e adicione (ou abra) a conta do YouTube dele.
2. Escolha **Conectar**. O navegador abre a página de consentimento do Google.
3. Entre com a conta dona do canal, escolha o canal se o Google pedir e permita todas as permissões. Deixar uma desmarcada faz o Bardo recusar a conexão e revogar o que foi dado.
4. O navegador mostra "O Bardo está conectado" e o cartão mostra **Conectada como (nome do canal)**.

O Bardo espera cinco minutos pelo navegador; **Cancelar** para de esperar.

Os tokens de acesso e de renovação ficam no Gerenciador de Credenciais do Windows, por perfil e por conta de rede. O banco de dados do Bardo guarda só o id e o nome do canal, os escopos, a validade do token e a última renovação.

<a id="day-to-day"></a>
## No dia a dia

- **Verificar** renova o acesso se ele estiver perto de expirar e lê de novo o nome do canal.
- **Reconexão necessária** quer dizer que o Google recusou renovar o acesso: ele foi revogado na sua conta do Google, a chave secreta do cliente mudou, ou o app ainda está em **Teste** e os sete dias acabaram. Escolha **Reconectar**.
- **Desconectar** revoga o acesso do Bardo no Google e esquece os tokens. Se não for possível falar com o Google, o Bardo esquece os tokens mesmo assim e pede que você remova o acesso dele nas configurações de segurança da sua conta do Google (**Apps e serviços de terceiros**).
- Uma conta conectada precisa ser desconectada antes de ser removida.

<a id="upload"></a>
## Enviar um vídeo

1. Renderize o projeto para a conta do YouTube e escreva os metadados.
2. Na etapa **Publicação** do projeto, escolha **YouTube** e **Revisar envio**. O botão fica desligado, com o motivo embaixo, enquanto a conta não está conectada, um render está em andamento ou desatualizado, ou os metadados faltam ou passam dos limites do YouTube.
3. A revisão mostra o arquivo, o canal conectado e o título, a descrição (com o rodapé da conta) e as tags do jeito que o YouTube os recebe. Escolha a visibilidade, responda **Conteúdo para crianças** e confira **Conteúdo alterado ou sintético** (já marcado quando a voz do narrador está marcada como realista).
4. Se o projeto já tem um post do YouTube vinculado, marque a caixa que o substitui no Bardo; o post continua no YouTube.
5. Escolha **Enviar**. Nada é enviado antes disso. Se o render, o corte ou os metadados mudaram desde que a revisão abriu, o Bardo a fecha e pede que você revise de novo.

O envio roda como tarefa: a seção Publicação mostra o progresso, depois **Processando** enquanto o YouTube trabalha no arquivo, e **Enviado** com o link do vídeo. **Parar** mantém o que o YouTube já recebeu; **Retomar** manda só o resto. Uma conexão que cai tenta de novo sozinha do mesmo ponto.

Enquanto o envio roda, o Bardo recusa renderizar o projeto, porque o render reescreveria o arquivo sendo enviado; **Retomar** espera um render em andamento do mesmo jeito. Se o projeto foi renderizado de novo com o envio parado, retomar encerra esse envio com "O render mudou depois da revisão": o resto do arquivo não é o que você revisou, então revise o envio de novo.

O Bardo acompanha o processamento do YouTube por cerca de uma hora. Se o YouTube ainda estiver processando o vídeo depois disso, o post mostra **Ainda processando**; escolha **Verificar de novo** mais tarde.

Colar o link de um post sobre um vídeo enviado pergunta antes e substitui o envio só no Bardo; o vídeo continua no YouTube.

<a id="schedule"></a>
## Agendamento

Na revisão, **Quando** oferece **Assim que processar** ou **Agendar**. Com **Agendar**, digite a data e a hora em que o YouTube torna o vídeo público; os campos as leem na ordem do idioma da interface (DD/MM/AAAA e 18:30 em português, MM/DD/YYYY e 6:30 PM em inglês) e no fuso horário do seu computador, que o cartão mostra. O Bardo envia o vídeo como privado com esse horário de publicação, e o YouTube o publica sozinho: o Bardo e o computador podem estar desligados. Um horário que já passou é recusado quando você confirma, porque o YouTube publicaria o vídeo na hora.

O post passa a mostrar **Agendado** com o horário em que fica público. Até lá, **Mudar horário** manda um horário novo e **Cancelar agendamento** deixa o vídeo privado no YouTube sem horário de publicação; publicá-lo depois é feito no YouTube Studio. Os dois reenviam a resposta de conteúdo para crianças e a declaração de conteúdo sintético que o YouTube já tem, porque o YouTube apaga o que uma mudança deixa de fora. Uma mudança feita no YouTube Studio aparece na próxima sincronização de métricas do Bardo.

Cada sincronização de métricas lê de volta um vídeo agendado pela conta conectada. Quando o YouTube o torna público, o post passa a **Publicado** com o horário de publicação do YouTube, e as métricas dele são lidas como as de qualquer outro post. A sincronização não precisa de chave de API enquanto só houver vídeos agendados.

<a id="private"></a>
## Envios ficam privados até a auditoria

O Google trava como **privado** todo vídeo enviado por um projeto de API sem auditoria criado depois de 28/07/2020, e os vídeos agendados também. O Bardo mostra essa publicação como **Mantido privado**, com "Mantido privado pelo YouTube: seu projeto do Google ainda não passou pela auditoria da API do YouTube". Um vídeo agendado que continua privado quinze minutos depois do horário de publicação aparece do mesmo jeito. Não é uma falha. Para publicar como público pelo Bardo, peça a [auditoria dos Serviços da API do YouTube](https://support.google.com/youtube/contact/yt_api_form) para o seu projeto; ela muda o resultado, nada no Bardo. Até lá, você pode tornar um vídeo público no YouTube Studio.

<a id="owner-metrics"></a>
## Métricas do dono

Com a conta do YouTube do canal conectada, toda sincronização de métricas também lê, da YouTube Analytics API, os números de dono de cada post do YouTube que encontra, vinculado à mão ou enviado: visualizações engajadas, visualizações, tempo de exibição, a visualização média (duração e porcentagem), a curva de retenção do público e, para um canal no Programa de Parcerias do YouTube, a receita estimada, o CPM e o CPM por reprodução. O RPM é calculado pelo Bardo como receita por 1.000 visualizações. As visualizações, curtidas e comentários públicos continuam vindo da chave da YouTube Data API, então a sincronização precisa da chave como antes.

- **Nenhuma permissão nova.** Os dois escopos `yt-analytics` estão entre os quatro que o Bardo pede na conexão, e uma conexão sem algum deles é recusada, então um canal conectado antes das métricas do dono as lê sem reconectar.
- **Visualizações engajadas na frente.** As visualizações do YouTube agora contam todo início de vídeo (Shorts desde março de 2025, todos os formatos desde agosto de 2026); as visualizações engajadas mantêm o sentido antigo, então são o número principal no post e em Estratégia › Desempenho.
- **Não monetizado.** Um canal fora do Programa de Parcerias recebe uma recusa nos relatórios de receita. O Bardo mostra **Não monetizado** no lugar deles e lê todo o resto.
- **Dois ou três dias de atraso.** Os dados da YouTube Analytics chegam de 48 a 72 horas depois; o post avisa isso embaixo dos números de dono. Um vídeo novo mostra só os números públicos até chegarem as primeiras análises.
- **Reconexão necessária.** Enquanto a conta precisa reconectar, as sincronizações continuam lendo os números públicos, e Estratégia › Desempenho pede que você reconecte. Um canal que nunca foi conectado mantém exatamente os números públicos que tinha.
- O desempenho passado no ranking de temas continua lendo as visualizações públicas: os números de dono chegam dias depois e os posts mais antigos de um canal podem não ter nenhum.

<a id="quota"></a>
## Cota

Os envios usam uma cota separada de 100 envios por dia por projeto; as outras chamadas dividem 10.000 unidades por dia. Conectar, verificar e ler de volta um vídeo agendado custam 1 unidade cada; mudar ou cancelar um agendamento custa 51 (uma leitura e depois a atualização). A YouTube Analytics tem cota própria, separada da Data API: cada sincronização pede a ela dois relatórios por post de um canal conectado (os números e a curva de retenção), uma unidade cada, mais um por sincronização para um canal não monetizado (o primeiro relatório de receita é recusado e então lido de novo sem receita). Quando a cota acaba, o Bardo avisa e o contador zera à meia-noite no horário do Pacífico. Um envio que atinge a cota diária de envios para na hora em vez de tentar de novo; escolha **Tentar de novo** depois que ela zerar.
